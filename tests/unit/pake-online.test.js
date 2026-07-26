import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import {
  applyOnlineReleaseVersion,
  createBootstrapCliConfig,
  createBuildConfig,
  createConfigId,
  createMatrix,
  loadRegistryConfigs,
  normalizeReleaseVersion,
  ONLINE_BOOTSTRAP_VERSION,
  pauseRegistryConfig,
  upsertRegistryConfig,
} from "../../scripts/pake-online/config.mjs";
import {
  isIco,
  normalizeIconUrl,
  writeWindowsIcon,
} from "../../scripts/pake-online/icon.mjs";
import {
  detectArtifactFormat,
  ONLINE_MANIFEST_SCHEMA_VERSION,
  selectReleaseAssetsToDelete,
  stageReleaseAssets,
} from "../../scripts/pake-online/release.mjs";

const temporaryDirectories = [];

function temporaryDirectory() {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "pake-online-"));
  temporaryDirectories.push(directory);
  return directory;
}

function sampleInputs(overrides = {}) {
  return {
    platform: "windows-latest",
    url: "https://example.com",
    name: "Example App",
    icon: "",
    width: "1200",
    height: "780",
    min_width: "",
    min_height: "",
    app_version: "1.2.3",
    fullscreen: false,
    hide_title_bar: false,
    multi_arch: false,
    targets: "deb",
    online_mode: true,
    online_operation: "enable-or-update",
    online_windows_format: "msi",
    offline_exe: false,
    offline_exe_icon: "",
    online_exe_icon: "",
    ...overrides,
  };
}

function sampleContext(overrides = {}) {
  return {
    repository: "owner/repo",
    sourceBranch: "main",
    runId: "42",
    now: "2026-07-23T00:00:00.000Z",
    ...overrides,
  };
}

function onlineConfig(osName, overrides = {}) {
  const platform = {
    windows: "windows-latest",
    macos: "macos-latest",
    linux: "ubuntu-24.04",
  }[osName];
  return createBuildConfig(
    sampleInputs({ platform, ...overrides }),
    sampleContext(),
  );
}

afterEach(() => {
  for (const directory of temporaryDirectories.splice(0)) {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

describe("Pake online build configuration", () => {
  it("creates a stable id from repository, branch, platform, and app name", () => {
    const value = {
      repository: "owner/repo",
      sourceBranch: "main",
      platform: "windows-latest",
      name: "Example App",
    };
    expect(createConfigId(value)).toBe(createConfigId(value));
    expect(createConfigId(value)).toMatch(/^example-app-windows-[a-f0-9]{10}$/);
    expect(createConfigId({ ...value, sourceBranch: "release" })).not.toBe(
      createConfigId(value),
    );
  });

  it("normalizes workflow values and independent Windows EXE icons", () => {
    const config = createBuildConfig(
      sampleInputs({
        min_width: "640",
        fullscreen: true,
        offline_exe: true,
        online_windows_format: "exe",
        offline_exe_icon: "https://example.com/offline.ico",
        online_exe_icon: "https://example.com/online.png",
      }),
      sampleContext(),
    );

    expect(config.online).toBe(true);
    expect(config.releaseTag).toBe(`pake-online-${config.id}`);
    expect(config.delivery).toEqual({
      windowsOfflineExe: true,
      onlineWindowsFormat: "exe",
      offlineExeIcon: "https://example.com/offline.ico",
      onlineExeIcon: "https://example.com/online.png",
    });
    expect(config.cliConfig).toMatchObject({
      url: "https://example.com",
      name: "Example App",
      minWidth: 640,
      appVersion: "1.2.3",
      targets: "msi",
      fullscreen: true,
    });
  });

  it("uses the latest release for payloads and a high bootstrap version", () => {
    for (const [osName, payloadTarget, bootstrapTarget] of [
      ["windows", "msi", "msi"],
      ["macos", "app", "dmg"],
      ["linux", "appimage", "appimage"],
    ]) {
      const runtime = applyOnlineReleaseVersion(
        onlineConfig(osName),
        "V3.15.1",
      );
      expect(runtime.cliConfig.appVersion).toBe("3.15.1");
      expect(runtime.cliConfig.targets).toBe(payloadTarget);
      expect(createBootstrapCliConfig(runtime)).toMatchObject({
        appVersion: ONLINE_BOOTSTRAP_VERSION,
        targets: bootstrapTarget,
      });
    }
    expect(ONLINE_BOOTSTRAP_VERSION).toBe("255.0.0");
    expect(normalizeReleaseVersion("v4.0.0-beta.1")).toBe("4.0.0-beta.1");
    expect(() => normalizeReleaseVersion("latest")).toThrow(/semantic version/);
  });

  it("keeps non-online versions and rejects invalid input", () => {
    const config = createBuildConfig(
      sampleInputs({ online_mode: false }),
      sampleContext(),
    );
    expect(applyOnlineReleaseVersion(config)).toBe(config);
    expect(config.cliConfig.appVersion).toBe("1.2.3");
    expect(() =>
      createBuildConfig(
        sampleInputs({ url: "file:///tmp/app.html" }),
        sampleContext(),
      ),
    ).toThrow(/http or https/);
    expect(() =>
      createBuildConfig(
        sampleInputs({ online_windows_format: "nsis" }),
        sampleContext(),
      ),
    ).toThrow(/Windows installer format/);
  });

  it("updates, filters, pauses, and emits an empty matrix safely", () => {
    const directory = temporaryDirectory();
    const initial = onlineConfig("windows");
    upsertRegistryConfig(directory, initial);
    upsertRegistryConfig(directory, {
      ...initial,
      updatedAt: "2026-07-24T00:00:00.000Z",
      createdAt: "changed",
    });
    upsertRegistryConfig(
      directory,
      createBuildConfig(
        sampleInputs({ name: "Release App" }),
        sampleContext({ sourceBranch: "release" }),
      ),
    );

    const configs = loadRegistryConfigs(directory, "main");
    expect(configs).toHaveLength(1);
    expect(configs[0].createdAt).toBe("2026-07-23T00:00:00.000Z");
    expect(createMatrix(configs).include[0].config.id).toBe(initial.id);
    expect(pauseRegistryConfig(directory, initial.id)).toBe(true);
    expect(pauseRegistryConfig(directory, initial.id)).toBe(false);
    expect(loadRegistryConfigs(directory, "main")).toEqual([]);
    expect(createMatrix([])).toEqual({ include: [] });
  });
});

describe("Pake online release assets", () => {
  it("recognizes only the compressed runtime payload formats", () => {
    expect(detectArtifactFormat("App.exe.zip")).toBe("exe.zip");
    expect(detectArtifactFormat("App.app.zip")).toBe("app.zip");
    expect(detectArtifactFormat("App.AppImage")).toBe("appimage");
    expect(detectArtifactFormat("App.msi")).toBeNull();
    expect(detectArtifactFormat("App.dmg")).toBeNull();
    expect(detectArtifactFormat("App.7z")).toBeNull();
  });

  it.each([
    ["windows", "application.exe.zip", "exe.zip", "app.exe", "executable"],
    ["macos", "Example App.app.zip", "app.zip", "Example App.app", "appBundle"],
    ["linux", "application.AppImage", "appimage", "app.AppImage", "executable"],
  ])(
    "stages one verified %s payload",
    (osName, fileName, format, entrypoint, launchKind) => {
      const root = temporaryDirectory();
      const input = path.join(root, "input");
      const output = path.join(root, "output");
      fs.mkdirSync(input);
      fs.writeFileSync(path.join(input, fileName), "compressed-payload");
      const config = onlineConfig(osName);
      const result = stageReleaseAssets(config, {
        inputDirectory: input,
        outputDirectory: output,
        sourceSha: "1234567890abcdef",
        runAttempt: "2",
        arch: "X64",
        builtAt: "2026-07-23T00:00:00.000Z",
      });

      expect(result.manifest.schemaVersion).toBe(
        ONLINE_MANIFEST_SCHEMA_VERSION,
      );
      expect(result.manifest.artifacts).toHaveLength(1);
      const payload = result.manifest.artifacts[0];
      expect(payload).toMatchObject({
        format,
        size: 18,
        packageId: "Example App",
        entrypoint,
        launchKind,
      });
      expect(payload.sha256).toMatch(/^[a-f0-9]{64}$/);
      expect(payload.name).toBe(
        `${config.id}-1234567890ab-${payload.sha256.slice(0, 12)}.${format}`,
      );
      expect(path.basename(result.manifestPath)).toBe(
        `pake-online-manifest-1234567890ab-${payload.sha256.slice(0, 12)}.json`,
      );
    },
  );

  it("uses the configured Windows online carrier format", () => {
    const root = temporaryDirectory();
    const input = path.join(root, "input");
    fs.mkdirSync(input);
    fs.writeFileSync(path.join(input, "application.exe.zip"), "payload");
    const config = onlineConfig("windows", {
      online_windows_format: "exe",
    });
    const result = stageReleaseAssets(config, {
      inputDirectory: input,
      outputDirectory: path.join(root, "output"),
      sourceSha: "1234567890abcdef",
      arch: "X64",
    });
    expect(result.manifest.onlineInstaller.name).toBe(
      `${config.id}-online-installer.exe`,
    );
  });

  it("rejects ambiguous payload directories", () => {
    const root = temporaryDirectory();
    fs.writeFileSync(path.join(root, "one.AppImage"), "one");
    fs.writeFileSync(path.join(root, "two.AppImage"), "two");
    expect(() =>
      stageReleaseAssets(onlineConfig("linux"), {
        inputDirectory: root,
        outputDirectory: path.join(root, "output"),
        sourceSha: "1234567890abcdef",
        arch: "X64",
      }),
    ).toThrow(/exactly one/);
  });

  it("does not overwrite a rebuilt payload from the same source commit", () => {
    const root = temporaryDirectory();
    const input = path.join(root, "input");
    fs.mkdirSync(input);
    const payload = path.join(input, "application.exe.zip");
    const config = onlineConfig("windows");
    const stage = (outputDirectory) =>
      stageReleaseAssets(config, {
        inputDirectory: input,
        outputDirectory,
        sourceSha: "1234567890abcdef",
        arch: "X64",
      });

    fs.writeFileSync(payload, "first build");
    const first = stage(path.join(root, "first"));
    fs.writeFileSync(payload, "changed configuration build");
    const second = stage(path.join(root, "second"));

    expect(second.manifest.artifacts[0].name).not.toBe(
      first.manifest.artifacts[0].name,
    );
    expect(path.basename(second.manifestPath)).not.toBe(
      path.basename(first.manifestPath),
    );
  });

  it("keeps the latest two manifests and their referenced assets", () => {
    const assets = [
      { id: 9, name: "pake-online-manifest-new.json" },
      { id: 8, name: "pake-online-manifest-previous.json" },
      { id: 7, name: "pake-online-manifest-old.json" },
      { id: 6, name: "example-windows-id-new.exe.zip" },
      { id: 5, name: "example-windows-id-previous.exe.zip" },
      { id: 4, name: "example-windows-id-old.exe.zip" },
      { id: 3, name: "example-windows-id-online-installer.msi" },
      { id: 2, name: "maintainer-notes.txt" },
    ];
    const manifests = new Map([
      [
        "pake-online-manifest-new.json",
        {
          artifacts: [{ name: "example-windows-id-new.exe.zip" }],
          onlineInstaller: {
            name: "example-windows-id-online-installer.msi",
          },
        },
      ],
      [
        "pake-online-manifest-previous.json",
        {
          artifacts: [{ name: "example-windows-id-previous.exe.zip" }],
          onlineInstaller: {
            name: "example-windows-id-online-installer.msi",
          },
        },
      ],
    ]);

    expect(
      selectReleaseAssetsToDelete(assets, manifests, "example-windows-id").map(
        ({ name }) => name,
      ),
    ).toEqual([
      "pake-online-manifest-old.json",
      "example-windows-id-old.exe.zip",
    ]);
  });
});

describe("Windows installer icon preparation", () => {
  it("accepts web URLs without embedded credentials", () => {
    expect(normalizeIconUrl("https://example.com/icon.png")).toBe(
      "https://example.com/icon.png",
    );
    expect(() => normalizeIconUrl("file:///tmp/icon.ico")).toThrow(/HTTP/);
    expect(() =>
      normalizeIconUrl("https://token@example.com/icon.ico"),
    ).toThrow(/credentials/);
  });

  it("preserves ICO and converts PNG input", async () => {
    const directory = temporaryDirectory();
    const icoOutput = path.join(directory, "installer.ico");
    const ico = Buffer.from([0, 0, 1, 0, 0, 0]);
    await writeWindowsIcon(ico, icoOutput);
    expect(isIco(fs.readFileSync(icoOutput))).toBe(true);

    const pngOutput = path.join(directory, "converted.ico");
    const png = fs.readFileSync(
      path.join(process.cwd(), "src-tauri/png/icon_512.png"),
    );
    await writeWindowsIcon(png, pngOutput);
    expect(isIco(fs.readFileSync(pngOutput))).toBe(true);
  });
});

describe("Build App With Pake CLI online workflow", () => {
  const workflowPath = path.join(
    process.cwd(),
    ".github/workflows/pake-cli.yaml",
  );

  it("registers push builds and uses the native Pake bootstrap", () => {
    const workflow = fs.readFileSync(workflowPath, "utf8");
    expect(workflow).toContain("workflow_dispatch:");
    expect(workflow).toContain("push:");
    expect(workflow).toContain("online_mode:");
    expect(workflow).toContain("online_operation:");
    expect(workflow).toContain('ONLINE_CONFIG_BRANCH: "pake-online-config"');
    expect(workflow).toContain("PAKE_ONLINE_BOOTSTRAP");
    expect(workflow).toContain("PAKE_WINDOWS_ONLINE_PAYLOAD");
    expect(workflow).toContain("pake-bootstrap-cli-config.json");
    expect(workflow).toContain("Compress-Archive");
    expect(workflow).toContain("application.exe.zip");
    expect(workflow).toContain("ditto -c -k");
    expect(workflow).toContain("& $light -sval");
    expect(workflow).toContain("Build offline EXE wrapper (Windows)");
    expect(workflow).toContain('if ($env:ONLINE_WINDOWS_FORMAT -eq "exe")');
    expect(workflow).toContain('$env:PAKE_INSTALLER_VERSION = "255.0.0"');
    expect(workflow).not.toMatch(/QtIFW|qtifw|binarycreator|repogen/);
    expect(workflow).not.toContain("pake-online-repository-worktree");
  });

  it("uses a UTF-8 MSI database for non-Latin application names", () => {
    const wixTemplate = fs.readFileSync(
      path.join(process.cwd(), "src-tauri/assets/main.wxs"),
      "utf8",
    );
    const wixLocale = fs.readFileSync(
      path.join(process.cwd(), "src-tauri/assets/en-US.wxl"),
      "utf8",
    );
    const windowsConfig = JSON.parse(
      fs.readFileSync(
        path.join(process.cwd(), "src-tauri/tauri.windows.conf.json"),
        "utf8",
      ),
    );
    expect(wixTemplate).toMatch(/<Product[\s\S]*Codepage="65001"/);
    expect(wixLocale).toMatch(/WixLocalization[\s\S]*Codepage="65001"/);
    expect(wixLocale).toContain('<String Id="TauriCodepage">1252</String>');
    expect(windowsConfig.bundle.windows.wix.language).toEqual({
      "en-US": { localePath: "assets/en-US.wxl" },
    });
  });

  it("publishes payload and bootstrap before the completed manifest", () => {
    const workflow = fs.readFileSync(workflowPath, "utf8");
    const actualUpload = workflow.indexOf(
      "for asset in .pake-online-release/actual/*",
    );
    const onlineUpload = workflow.indexOf(
      "for asset in .pake-online-release/online/*",
    );
    const manifestUpload = workflow.indexOf(
      "for manifest in .pake-online-release/manifest/*",
    );
    expect(actualUpload).toBeGreaterThan(0);
    expect(onlineUpload).toBeGreaterThan(actualUpload);
    expect(manifestUpload).toBeGreaterThan(onlineUpload);
  });

  it("keeps manual artifacts unchanged and publishes only the online bootstrap", () => {
    const workflow = fs.readFileSync(workflowPath, "utf8");
    for (const runner of ["Windows", "macOS", "Linux"]) {
      expect(workflow).toContain(
        `if: runner.os == '${runner}' && !matrix.config.online`,
      );
    }
    expect(workflow).toContain("- name: Upload online installer");
    expect(workflow).toContain(
      "name: ${{ matrix.config.cliConfig.name }}-${{ matrix.config.os }}-online-installer",
    );
    expect(workflow).toContain('path: ".pake-online-release/online/*"');
  });

  it("compiles and tests the bootstrap on all platforms in full CI", () => {
    const workflow = fs.readFileSync(
      path.join(process.cwd(), ".github/workflows/quality-and-test.yml"),
      "utf8",
    );
    expect(workflow).toContain("online-installer-build:");
    expect(workflow).toContain("Test online bootstrap");
    expect(workflow).toContain("Build release online bootstrap");
    expect(workflow).toContain("--features online-bootstrap");
    expect(workflow).not.toMatch(/QtIFW|qtifw|binarycreator|repogen/);
  });
});
