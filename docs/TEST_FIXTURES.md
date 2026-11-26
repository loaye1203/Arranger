# 测试样本

源码附带 Rust 测试所需的 CDR 交换控制文件和项目生成的合成图片。控制文件保留用于验证几何、文字、颜色和资源往返的原生记录。原始作者字段、来源未核实的 ICC 配置和不透明的嵌入数据已清理。

`examples/assets/cdr/rust14-images/jpeg-profile.jpg` 保留内嵌的 Little CMS 内置 sRGB 配置，用于验证 JPEG 元数据保留。该配置自身的版权字段允许自由使用。

