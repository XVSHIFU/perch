# 01 产品边界与源码复用

## 1. 已确认的产品方向

- 名称：栖点 / Perch；图标：用户选择的第二张鸟类方案 01（尖头羽、单腿站姿、双尾羽）。当前使用 PNG/ICO，未完成路径矢量化，不自行重新设计。
- Windows 优先。建议首发验证 Windows 11 x64；Windows 10、ARM64、macOS、Linux 单独列验证矩阵，不因 Tauri 跨平台就声称已支持。
- 桌面管理端：Tauri 2 + React + TypeScript + Rust。保留已有 Vite 工程。
- DSH 工作台：上游原生 DSH Web。
- Pi 工作台：优先验证 agegr/pi-web，适配层应允许后续替换。
- 默认在系统浏览器打开工作台；内嵌窗口为后续版本，不作为首版前置工作。
- 固定并验证“运行时 + 引擎 + Web + 插件”的组合；不要每次启动拉取 latest。
- 借鉴 PCL 的实例、版本、Mod/整合包心智；当前并未基于 PCL 源码套壳，不引入第二套 .NET UI。

## 2. UI 基线

以现有代码和 HTML 为准，保留：B 的左侧导航、项目工作区、环境详情、就地错误；山景启动台；主从实例列表；C 的四张整合包插画、详情抽屉、组合构建；空白创建和从整合包创建；全局底部状态栏；搜索 Ctrl K、收藏、克隆、版本比较、恢复中心、导入导出。

窄窗口中右侧组合区收到底部，关键操作仍能到达。主区域独立滚动，导航和运行状态常驻。原对话的“1A、2A、3C、4按建议、5A、6B、7按建议、8底栏、9–14保留”已有上述可见实现，编号没有独立定义表，不允许根据编号臆造新需求。

初次打开正式版应显示真实空状态和创建入口；演示目录放入独立 demo 模式，不把样例实例当成用户机器已有实例。市场首版可使用人工维护的目录，UI 展示来源、版本、验证状态、平台限制、安装对象与预计变更。

## 3. 复用审计

| 文件/模块 | 当前事实 | 开发动作 |
| --- | --- | --- |
| `src/App.tsx` | 已实现交互流，页面集中在单文件，有 any、内嵌渲染函数、前端状态模拟 | 保留视觉；先拆页面/组件和 gateway，再换真实接口 |
| `src/styles.css` | 包含旧方案与桌面覆盖样式 | 提取 token 和组件样式，视觉回归后清理旧规则，避免大规模换样式 |
| `src/PerchMark.tsx`、`bird-mark.json` | 内联 PNG 标志，暗色反白 | 可直接沿用，后续改资源引用不影响形状 |
| `brand/bird`、`src-tauri/icons` | 当前图标；brand 根下还留历史 Q 素材 | 保留小鸟；历史素材归档，避免重新引用 |
| `src/model.mjs` | 示例目录、简单引擎过滤、JSON 校验、数组恢复 | 将示例移 fixtures；规则以正式 profile 校验替代 |
| `src/native.ts` | invoke/listen 封装，只有演示服务命令 | 可保留接口入口，补类型和正式命令；不让页面直接散落 invoke |
| `src-tauri/src/main.rs` | 托盘/窗口 + Rust 线程中的静态 HTTP 服务 | 保留 shell 能力；demo server 与真实 ProcessSupervisor 分离 |
| `src-tauri/tauri.conf.json` | 无边框窗口、最低尺寸、NSIS、CSP | Windows 实测、修复后沿用，不删除 CSP 来掩盖问题 |
| `build-windows.ps1` | 前置命令检查、测试、Tauri 构建 | 补版本诊断；退出码失败应终止 |
| `.github/workflows/windows-build.yml` | 假设 app 是仓库根目录 | 若保留交接包根目录，须给 npm 步骤设 app working-directory，cache 指向 app/package-lock.json，制品路径加 app/ |
| `tests/model.test.mjs` | 4 项纯逻辑测试已过 | 保留有价值的隔离/导入约束，随 domain 重构迁移 |
| `verification/ui.test.mjs` | 未执行通过，jsdom 不在锁定依赖中 | 不把它当 gate；按需要安装测试依赖修复或改 Playwright |
| `dist`、单文件 HTML | 构建产物/界面基线 | 不直接修改压缩 bundle |

## 4. 必须纠正的样机语义

- 示例 Pi 0.87.1、Pi Web 0.9.3 不是本项目认证过的兼容组合。实际版本从验证 profile 导入。
- 示例 Skill 标记“通用”只表示类型过滤，不意味着跨引擎执行一致；需要适配器与实际测试。
- “ready”现在只是清单状态；正式版拆分安装状态和运行状态，端口通并不代表 Agent 健康。
- 恢复现在只覆盖对象；正式恢复必须事务化，保留旧版本与配置，不静默覆盖用户项目。
- UI 里的路径如 D:\Projects 只是展示，不证明磁盘目录存在。
- “独立实例”表示数据与配置隔离；默认不是 OS 沙箱，不能宣称插件无权访问其他目录。
- 关闭到托盘与退出不同：托盘保留进程；退出按产品策略停止所管进程树。不得结束用户在外部启动的同名程序。

## 5. 第一版包含与后续工作

首版：两个引擎至少各一套验证组合；创建/安装/启动/停止/日志；项目选择；固定插件安装与禁用；组合目录和本地 JSON 导入导出；克隆、变更预览、更新前快照、恢复；Windows 安装卸载、托盘、主题、状态与异常处理。

后续：内嵌工作台、社区自动投稿与审核、账号/云同步、远程机器、多平台发布、自动更新客户端、商店签名发布。首版不承诺任意插件跨生态兼容，不提供任意地址的一键执行入口。

CI 迁移补充：若仓库根为本交接包根目录，GitHub 只发现根目录 `.github/workflows`。应将 `app/.github/workflows/windows-build.yml` 移到根目录，再设置 `defaults.run.working-directory: app`、`cache-dependency-path: app/package-lock.json` 与 `app/src-tauri/target/...` 制品路径。不能只修改 working-directory 却保留嵌套 workflow。
