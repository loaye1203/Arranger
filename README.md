# Arranger

Arranger 是本地运行的二维矢量编辑器。使用毫米单位绘制多页矢量图与编排文字，保存为可编辑的 `.arranger` 原稿，或导出 SVG、PNG 和 PDF。支持导入和导出兼容的 CDR 14 / 21 文件。CDR 导出提供保编辑和保外观两种模式，导出前会检查对象是否可表示。文字排版受系统字体影响。

## 从源码构建

需要 Rust 1.99 或更新版本。Windows x64 需要 MSVC C++ Build Tools 和 Windows SDK，首次构建可在 x64 Native Tools 命令提示符里运行。Apple Silicon macOS 需要 Xcode Command Line Tools。首次构建需要获取 `Cargo.lock` 锁定的依赖及 Skia 构建所需的本机库。

```sh
cargo build --workspace --release --locked
cargo run --locked -p arranger -- gui
cargo run --locked -p arranger -- describe
```

只构建命令行程序时，为 Cargo 添加 `--no-default-features`。在 Windows 上使用 `target/release/arranger.exe`，其他平台使用 `target/release/arranger`。首次运行测试前，在仓库根目录创建 `artifacts` 目录，再执行 `cargo test --workspace --locked`。少数文字测试需要测试名称指定的系统字体。样本说明见 [测试样本](docs/TEST_FIXTURES.md)。

如果已有 Python 3，可以生成一份自制的双页演示文档。用一个空目录存放输出：

```sh
python examples/v1_design.py demo-output
cargo run --locked -p arranger -- run demo-output/requests.jsonl
cargo run --locked -p arranger -- gui --open demo-output/vector-study.arranger
```

命令协议见 [API 说明](docs/API.md) 和 [JSON Schema](docs/api-v1.schema.json)。Apple Silicon 安装镜像可在 macOS 上用 `python3 scripts/package_macos_preview.py dist` 构建。脚本生成带本地签名的体验镜像。

## 许可证

项目自有源码使用 [MIT](LICENSE)。第三方组件、图标及内置 UI 字体沿用各自的许可证，通知文本见 [NOTICE](NOTICE) 和 [第三方许可目录](docs/third-party-licenses/)。

