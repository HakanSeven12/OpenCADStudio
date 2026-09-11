# Windows Web CAD 发布

在仓库根目录双击 `publish_web.bat`，或在终端执行：

```bat
publish_web.bat --no-pause
```

脚本编译主程序和解析 Worker，再同步到：

```text
D:\GitProjects\shipontology.local\src\ShipOntology.WebCad\wwwroot\opencadstudio
```

宿主项目使用 `/app/` 提供这些静态文件，`/webcad/` 会跳转到此地址。
发布后刷新浏览器。脚本不会启动或重启 .NET 宿主。

需要安装 Rust、`wasm32-unknown-unknown`、Trunk 和与 Cargo.lock 匹配的
wasm-bindgen-cli。当前锁定的 wasm-bindgen 为 0.2.108：

```powershell
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
cargo install wasm-bindgen-cli --version 0.2.108 --locked
```

构建失败时会返回非零退出码，只有主程序、Worker 和必需资源完整生成后才同步。
同步仅限 `wwwroot\opencadstudio`，保留宿主源代码和其他静态资源。
PowerShell 实现在 `scripts/publish-web.ps1`，可通过 `-ProjectRoot` 指定另一份宿主项目。

# 大图打开修复

35.1 MiB 的实际 DWG 样本包含 197,783 个实体、345,765 个对象。
底层 DGN 线型读取器将无法解码的数量字段截为 100,000 后继续读到记录末尾之外，
生成大量无效数组；未压缩的文档序列化达到 3,844,838,236 字节。
WebAssembly 内存同时容纳解析文档、传输数据和主线程解码结果时容易耗尽。

修复包含：

- 按记录剩余位数校验 DGN 数组数量，无法解码的记录保留原始 DWG 数据。
- Worker 使用带缓冲的流式压缩，传输后先释放解析 Worker，再在主线程解码。
- 实体检查、派生缓存、块定义和首帧线框准备分批让出浏览器事件循环。
- 普通读取告警不再触发修复；记录丢失、截断和非法实体仍保留修复流程。
- 最近文件写入在后台完成，不再阻塞文档安装。

解析器修复保存在 `vendor/cadcodec`，来源与补丁说明见其
`OPENCADSTUDIO-PATCHES.md`。应用和 Worker 使用同一份依赖，避免协议两端不一致。

针对私有 DWG 的数据回归可手动运行，图纸不会被修改：

```powershell
$env:OCS_TEST_DRAWING = 'D:\drawings\example.dwg'
cargo test -p ocs_web_worker --release inspect_local_drawing -- --ignored --nocapture
```

正常回归：`cargo test -p ocs_web_worker`。
