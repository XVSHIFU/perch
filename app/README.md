# 栖点 / Perch · v1.0.0

使用说明、下载与功能范围见[项目首页](../README.md)。源码采用 React / TypeScript + Tauri / Rust；真实安装、文件与进程状态由 Rust 管理。

## 本地开发与构建

```powershell
npm ci
npm run desktop:dev
# 生成独立 EXE 和 NSIS 安装包
npm run desktop:build
```

不要用裸 `cargo build --release` 交付桌面应用，它可能沿用开发配置。`npm run dev` 仅启动前端开发服务器，不能替代桌面的真实后端。

```powershell
npm test
cargo test --locked --manifest-path src-tauri/Cargo.toml
```

Rust 中标记 ignored 的真实下载/安装验收不在默认测试中执行；需单独提供隔离环境并明确选择。Node 中直接检查已安装 Pi 的测试依赖本机 `verification/p1/pi/` 夹具，仅设置 `PERCH_TEST_INSTALLED_PI=1` 且夹具存在时执行，否则报告跳过，不额外下载依赖。`tests/verify-*.mjs` 是历史手动集成入口，不是默认测试。

## 目录

- `src/`：正式界面与桌面命令适配；历史样机为显式演示入口，正式失败不回退演示。
- `src-tauri/src/`：持久化、凭据、精确组合安装、恢复、引擎进程与来源适配。
- `src-tauri/catalog-seed/`：随二进制内置的公开资源目录快照，保留时间和完整性信息。
- `../docs/profiles/`：编译所需的引擎默认 profile、package 与锁文件，不可在源码发行时省略。
- `brand/`、`src-tauri/icons/`：选定的小鸟与标识；`scripts/build-bird-icons.py` 使用原始归档重建图标，需要 Pillow。
- `THIRD_PARTY/`：前端运行时与图标的第三方许可说明。

保持 `dev.perch.studio.lab` 标识和 `Perch Studio Lab` 安装名称，以复用原工作空间与升级路径。版本号同步维护 package.json、package-lock.json、Cargo.toml、Cargo.lock 和 tauri.conf.json。

公开版本不包含开发者的截图、私有配置、验收运行目录、node_modules 或构建产物。运行数据在用户应用数据目录，API Key 在 Windows 凭据管理器，不在源码或组合分享包中。
