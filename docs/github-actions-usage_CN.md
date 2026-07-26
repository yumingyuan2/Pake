# GitHub Actions 使用指南

<h4 align="right"><strong><a href="github-actions-usage.md">English</a></strong> | 简体中文</h4>

无需本地安装开发工具，在线构建 Pake 应用。

## 快速步骤

### 1. Fork 仓库

[Fork 此项目](https://github.com/tw93/Pake/fork)

### 2. 运行工作流

1. 前往你 Fork 的仓库的 Actions 页面
2. 选择 `Build App With Pake CLI`
3. 填写表单（参数与 [CLI 选项](cli-usage_CN.md) 相同）
4. 点击 `Run Workflow`

   ![Actions 界面](https://raw.githubusercontent.com/tw93/static/main/pake/action.png)

### 3. 下载应用

- 绿色勾号 = 构建成功
- 点击工作流名称查看详情
- 在 `Artifacts` 部分下载应用

  ![构建成功](https://raw.githubusercontent.com/tw93/static/main/pake/action2.png)

### 4. 构建时间

- **首次运行**：约 10-15 分钟（建立缓存）
- **后续运行**：约 5 分钟（使用缓存）
- 缓存大小：完成时为 400-600MB

### 可选的 Windows 离线 EXE

勾选 `offline_exe` 会额外发布一个 `.exe`。它内嵌本次生成的 MSI，并使用原生
Windows Installer 界面执行安装；原本的 MSI 离线包仍会保留。

`offline_exe_icon` 与 `online_exe_icon` 可分别设置离线 EXE 包装器和实验性
Windows 在线安装器的图标 URL。ICO 会直接使用；SVG、PNG、JPEG 以及其他 Sharp 支持的图片
会自动转换成 ICO。图标 URL 必须使用 HTTP(S)、不得包含凭据，大小上限为 10 MiB。

## 实验性在线模式

运行 `Build App With Pake CLI` 时勾选 `online_mode`，即可为当前分支登记本次
表单配置。首次运行会立即构建；以后每次向同一分支 push，都会重新构建该分支
登记的全部配置，并更新各自的滚动预发布。

每次在线模式构建都会自动把应用版本设为 Pake 最新正式 Release 的版本。在 Fork
中，工作流会读取其上游父仓库的最新 Release；非在线构建仍使用表单中的
`app_version`。

预发布会同时提供压缩后的真实应用负载和轻量在线引导程序。引导程序是一个完全
无窗口的 Pake 构建，不是第二个 Tauri 工程，也不依赖外部安装框架。它使用与
离线构建相同的 Tauri 原生打包器，因此应用名、图标、快捷方式和安装页面均与
离线包保持一致。

- Windows：`online_windows_format` 可选择应用专属 `.msi` 或 `.exe`。MSI
  就是正常的 Pake WiX 安装包；EXE 是对同一个 MSI 的完全无窗口包装，
  `online_exe_icon` 控制包装器图标。引导程序版本固定为 `255.0.0`，真实应用
  负载仍使用 Pake 最新正式 Release 版本。
- macOS：`.dmg` 由正常的 Pake DMG 打包器生成，其中的应用使用配置的名称和图标。
- Linux：`.AppImage` 由正常的 Pake AppImage 打包器生成，并使用配置的名称和图标。

首次启动应用时，引导程序会静默下载、校验并启动最新的真实应用。以后启动时会
立即打开本地缓存的应用，同时在后台检查更新；下载完成的新版本会在下次启动时
启用。整个过程不会显示控制台、更新进度窗口或第二套安装界面。

在线模式运行时，Actions 的 **Artifacts** 区域只提供在线安装引导器；如需真实
负载或原生软件包，请打开对应的滚动预发布。非在线模式仍只上传常规离线包，并
保留 3 天。

Windows 真实负载使用 ZIP 压缩的可执行文件，macOS 使用压缩后的应用包，Linux
保留本身已压缩的 AppImage。引导程序只接受其内置仓库和滚动 Release tag 下的
HTTPS 资产，并在启用前校验 manifest、字节数和 SHA-256。滚动 Release 只保留
当前和上一个成功负载及 manifest。

下载 GitHub 资产前，引导程序会查询 Cloudflare 国家信息。在中国大陆时优先使用
`https://v4.gh-proxy.org/https://github.com/owner/repo/releases/download/...`，
失败后回退到 GitHub；其他地区直接使用 GitHub。

### 前置条件与限制

- 在线模式为实验性功能，仅支持公开 Fork；配置和安装器绝不会保存 GitHub
  token。
- 在 **Settings → Actions → General → Workflow permissions** 中允许工作流
  读写仓库，以便维护配置分支和预发布。
- 配置按“应用名、平台、源码分支”区分；对同一组合再次选择
  `enable-or-update` 会更新已保存配置。
- 在同一应用、平台和分支下选择 `pause` 可停止后续 push 自动构建；最后一次
  预发布仍然保留。
- 配置保存在 `pake-online-config` 分支。每套配置都会在匹配分支的每次 push
  中占用一个 runner。
- 原生安装包会正常安装引导程序；三个平台下载的真实应用均保存在当前用户的
  Pake 数据目录。

## 提示

- 首次运行需要耐心等待，让缓存完全建立
- 建议网络连接稳定
- 如果构建失败，删除缓存后重试

## 链接

- [CLI 文档](cli-usage_CN.md)
- [高级用法](advanced-usage_CN.md)
