# P1 固定组合验证

2026-09-24，Windows 11 x64，Node 24.18.0、npm 12.0.2。本目录记录 P1 的固定组合验证，现由正式安装适配器嵌入使用。下表为历史验证范围，最新实现与验收状态见根目录 PROGRESS.md；不表示所有社区资源均已验证。

| 项目 | DSH | Pi Web |
| --- | --- | --- |
| 固定包 | @deepseek-ai/dsh@0.1.5-rc.3 | @agegr/pi-web@0.9.3 |
| Pi SDK | 不适用 | @earendil-works/pi-* 0.87.1 |
| 许可证 | MIT | MIT |
| 数据目录变量 | DSH_HOME | PI_CODING_AGENT_DIR |
| 双实例回环监听、中文/空格 cwd | 通过 | 通过 |
| 浏览器真实工作台 | 登录令牌入口通过 | HTTP 200，界面通过 |
| 设置隔离 | A 引导设置写入，B 无对应文件 | A PowerShell 工具设置写入，B 无对应文件 |
| 停止 A，B 保持运行；A 重启 | 通过（强制停止受控进程树） | 通过（强制停止受控进程树） |
| 会话隔离 | 用户真实会话保存于 A，B 无会话文件 | 用户真实会话和离线样本保存于 A，B 无会话文件 |
| 原生工作区选择 | 用户补测通过，保存路径核对一致 | 浏览器路径选择通过 |

## 固定来源与复现

每个子目录保存 package.json、完整 package-lock.json 和 profile.json。profile 包含注册表工件 URL、SHA-512 integrity、锁文件 SHA-256、入口及参数。DSH 的传递依赖存在版本范围，因此必须保留完整 lock，不能仅固定顶层包。将对应两个 package 文件复制到独立测试安装目录，执行：

```powershell
npm ci --ignore-scripts --no-audit --no-fund
```

先以固定版本 `npm install --save-exact --ignore-scripts` 生成锁文件，随后在新的 repro/dsh、repro/pi 目录执行上述 npm ci，分别安装 518、155 个包，锁文件摘要保持一致。这是当前开发机的新目录复现，不是干净 Windows 验收。不要覆盖主应用依赖。两个实例共用安装工件，各有独立 data 和项目目录；以项目目录为 cwd，使用安装目录中的绝对入口路径。

收尾重启使用同一目录和端口。两边真实对话经浏览器刷新后恢复，B 均无会话文件；凭据文件摘要未变，不输出其内容。Pi 所检查的 7 个文件全部不变；DSH 会话归档/索引发生变化，但真实历史回复成功恢复。结果见 restart-results.json。没有在重启后额外发送付费模型请求，因此凭据保留不等于重新调用认证已验证。

停止已验证受控 PID 的强制进程树清理。温和退出、活动模型任务中断与异常退出清理移交 P3/P4 Supervisor 实现时验证，不能直接宣称上游 Windows 信号处理可靠；干净机器安装归 P7。真实凭据重启后的再次请求尚未测试，作为 profile 限制保留。共享模型连接按 P2–P4 计划实现，不从测试目录自动导入用户密钥。

DSH 设置 DSH_HOME 为实例 data 的绝对路径、DSH_TELEMETRY_DISABLED=1，启动参数为 `web --host 127.0.0.1 --port <port> --no-open`。Pi 设置 PI_CODING_AGENT_DIR、PI_WEB_SKIP_VERSION_CHECK=1、NEXT_TELEMETRY_DISABLED=1，参数为 `--hostname 127.0.0.1 --port <port> --no-open`。入口见各自 profile。测试子进程只继承必要 Windows 环境变量，没有继承模型 API 密钥，没有修改全局 HOME。

## 接入时需要保留的结论

- DSH 首次入口含本地登录令牌，认证后进入根页面；未认证 GET / 返回 404，不能直接据此判断启动失败。日志和 .credentials.yaml 含凭据，不纳入 profile 或导出清单。DSH 会在自身 data/profiles 下准备模块代理目录，本轮没有据此认定它会自动 npm 下载。
- Pi CLI 会派生 Next 子进程，停止时需覆盖整个所属进程树。HTTP 200、Pi Web 页面及 /api/sessions 可辅助识别健康状态，但正式健康探测尚未实现。端口变化后浏览器项目选择不随服务器设置自动迁移。
- 安装脚本全部跳过；profile 列出 lock 中带脚本的依赖，不表示全部工具功能可用。Pi 的 Windows node-pty 预编译模块能加载，实际 cmd 输出标记并以 0 退出；测试父进程仍有句柄，已单独清理，不能算温和退出通过。
- 只对核实命令行的测试 PID 执行进程树强制停止，验证目标端口释放且另一实例仍监听。没有按进程名称批量终止。用户补测后 DSH A 也已清理，确认端口释放。
- Pi 离线会话样本明确标记为存储测试，只含测试用户文本，没有伪造模型回复。设置及样本结果见 isolation-results.json。随后用户在重新启动的 DSH 58659 / Pi 65038 中配置 API，并确认两边均可正常回复；真实模型调用记录为用户实测通过，非自动化测试。服务目前保持运行。后续重启验证补充见上文；再次模型认证、运行中模型任务中断、温和退出及干净机器安装仍未验证。

## 上游参考

- [DSH 官方仓库](https://github.com/deepseek-ai/deepseek-harness)
- [DSH 用户指南](https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/user/guide/index.md)
- [Pi Web 官方仓库](https://github.com/agegr/pi-web)

参数与目录行为同时核对本次固定版本的已安装源码；后续上游主分支变化不自动改变这里的组合。
