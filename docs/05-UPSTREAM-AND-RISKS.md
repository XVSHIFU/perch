# 05 上游核对、待验证项与决策记录

核对日期 2026-09-24；页面内容可能继续变化。这里只记录已读取的公开说明，**不等于本地安装验证**。具体生产版本必须在 Windows 固定 commit/package version 后复核。

## 已核对信息

| 上游 | 本次读取的事实 | 对开发的影响 |
| --- | --- | --- |
| deepseek-ai/deepseek-harness | README 提供 `npx @deepseek-ai/dsh web`；默认回环 3080；支持 --no-open；明确处于开发预览且可能破坏兼容 | 适配器统一浏览器打开；精确锁版本，不能追 latest |
| agegr/pi-web | README 要求 Node ≥22.19.0；CLI 包 @agegr/pi-web；支持端口/主机、--no-open；默认回环 30141；PI_CODING_AGENT_DIR 可覆盖数据目录 | 验证实例配置隔离、Pi 依赖版本、进程拓扑；Node 22 大版本还不够，必须核对补丁下限 |
| Tauri prerequisites | 已打开官方环境准备页 | Windows 构建链按官方当前要求准备与记录 |
| Congser/PCL-DSH | 本次页面读取失败，未核实代码、许可证和实现细节 | 仅保留为用户提出的参考，不依赖其未知 API 或复制其代码 |

参考链接：
- https://github.com/deepseek-ai/deepseek-harness
- https://github.com/agegr/pi-web
- https://v2.tauri.app/start/prerequisites/
- https://v2.tauri.app/learn/system-tray/
- https://v2.tauri.app/distribute/windows-installer/

用户提供、供后续定向核对的参考：
- https://github.com/Congser/PCL-DSH
- https://github.com/PCL-Community/PCL-CE
- https://github.com/Meloong-Git/PCL
- https://github.com/topics/dsh-plugin
- https://dsh-market.com/

后五项本次未完成内容/API/许可证核实。不要把示例市场条目当成正式适配完成，也不要将 UI 类比误写为代码继承关系。

## 开发前必须验证的问题

| 问题 | 验证方法 | 未通过时 |
| --- | --- | --- |
| DSH 独立数据目录与端口参数 | 对精确版本查 --help/源码，并运行两实例 | 阻止多实例正式能力，不改全局 HOME 掩盖 |
| Pi Web 的 Pi 依赖关系 | package lock、入口和运行进程核对 | 不允许用户任意组合两个版本 |
| Windows 原生依赖/安装脚本 | 干净 VM 安装固定工件 | 列缺失依赖或选兼容 profile，不默默引入 WSL |
| graceful stop 与孙进程 | 真实任务下停止与异常退出 | 完成 Job/进程组覆盖再验收 |
| 健康探针与鉴权 | 固定版本 endpoint/响应验证 | 暂不使用“已验证”状态 |
| 上游模型凭据存储 | 测试账号/临时目录，核对 auth 文件 | 明确落盘范围并排除导出 |
| 插件禁用是否需重启 | 实际装/卸载及重新启动测试 | 首版统一停机变更 |
| 第三方源再分发条件 | 对具体工件查 LICENSE/NOTICE | 仅链接上游或禁用镜像，不假设可打包 |
| 市场是否有稳定 API | 官方说明和实际响应 | 首版人工维护目录 |

## 已作架构决策

D01：Tauri 管理端 + 原生 Web 工作台，避免维护两套聊天 UI。
D02：先 Windows、默认浏览器；内嵌不阻塞首版。
D03：独立实例采用受管配置目录和固定 profile；隔离不等同安全沙箱。
D04：Rust 是本机真实状态权威，React 是投影；demo 明确隔离。
D05：整合包是可重建配方与锁文件，不是把整个用户目录压缩分享。
D06：不复刻 PCL 的底层 GUI，不在没有许可证核对时复用其源码。
D07：首版维护有限验证目录；社区市场服务、自动更新与远程管理后置。
D08：保留已选 UI/小鸟图标，技术重构不得自行换视觉语言。

## 最容易走错的方向

只把命令字符串放入按钮；用 npm latest 代替版本管理；把进程创建当启动成功；用端口连通代替服务身份；把 localStorage 当进程事实；把“插件当 Mod”理解为跨引擎直接互装；以“Windows 能跑网页”代替 Windows 进程树/文件/托盘验收；一开始就做云市场、账号和内嵌浏览器。
