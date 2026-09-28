# 栖点 / Perch

为 DSH 与 Pi 管理独立工作环境的 Windows 桌面应用。当前版本：**v1.0.0 · 首个正式版**。

<img src="app/src-tauri/icons/128x128.png" alt="栖点小鸟" width="80" height="80">

## 下载与开始使用

从 [GitHub Releases](https://github.com/XVSHIFU/perch/releases) 获取 Windows x64 安装包或独立 EXE。软件需要 Microsoft Edge WebView2；当前引擎安装/运行基线为 Node.js 24.18.0、npm 12.0.2，请确保可从命令行访问。

1. 在设置中添加模型连接。API Key 保存在 Windows 凭据管理器，同一个连接可供多个实例复用。
2. 选择整合包，或新建实例并选定项目目录、引擎和模型连接。
3. 安装后启动工作台；DSH / Pi 工作台在默认浏览器中打开。
4. 在扩展市场选择资源与安装目标，预览变更后安装。更改运行中的实例前先停止工作台。

## 主要功能

- **独立实例**：各自保存组合、配置和会话；支持克隆试用、删除后恢复和恢复点。
- **引擎版本管理**：历史版本及预览版目录，安装前解析精确依赖与兼容要求；从创建入口和实例版本管理选择。
- **统一扩展市场**：Pi 官方目录、npm、DSH 工坊与 GitHub 社区资源；支持 Skills、插件、主题、桌宠和预设的对应安装路径。
- **自定义整合包**：修改模板、编辑组合、锁定依赖、导入导出和新建实例。分享包不包含模型密钥。
- **来源与介绍**：内置可离线浏览的目录快照，支持手动更新；原生网页抽屉、作者中英文 README、图片及悬浮章节目录。

## 数据与兼容范围

实例数据保存在 `%APPDATA%/dev.perch.studio.lab/workspace/`。保留原有应用标识和安装名称 `Perch Studio Lab`，用于已有版本升级；不要手动删除该目录来更新软件。卸载默认保留数据，外部项目文件不随实例删除。

- 首版面向 Windows x64；暂未提供其他平台的发行包。
- 目录收录不等于所有历史版本或第三方资源已通过运行验证。安装前以资源声明、兼容检查和变更预览为准。
- 内置目录是带时间的快照，其中部分来源尚未完整收录，可继续同步。引擎和扩展安装仍需下载对应工件。
- 当前发行文件未进行代码签名；第三方资源、商标及许可证属于各自作者。
- 目前没有应用自动更新功能，后续版本从 Releases 下载。

## 从源码运行

需要 Node.js、npm、Rust stable，以及 Visual Studio C++ 桌面构建工具和 Windows SDK。

```powershell
cd app
npm ci
npm run desktop:dev
```

构建独立 EXE 与安装包：

```powershell
cd app
npm run desktop:build
```

产物位于 `app/src-tauri/target/release/perch-desktop.exe` 和 `app/src-tauri/target/release/bundle/nsis/`。使用 Tauri 构建入口，不以裸 `cargo build --release` 作为发行构建。

`app/build-windows.ps1` 可执行依赖安装、默认测试和构建；GitHub Actions 提供手动 Windows 构建。默认测试不会自动安装 DSH/Pi 或请求模型；依赖本机 Pi 安装夹具的可选测试在缺少夹具时跳过。

## 项目资料

- [开发说明](app/README.md)
- [实施计划与阶段边界](docs/03-IMPLEMENTATION-PLAN.md)
- [引擎固定组合资料](docs/profiles/README.md)
- [第三方说明](app/THIRD_PARTY/README.md)

历史截图、临时验收环境与交接包保留在开发者本机，不纳入源码仓库。
