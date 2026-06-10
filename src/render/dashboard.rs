/// 仪表盘渲染模块。
///
/// 无参数运行 byk 时显示完整仪表盘（banner + 帮助信息）。

use crate::core::paths::PathLayout;

/// 渲染完整仪表盘：banner + 帮助信息。
pub fn render(layout: &PathLayout, options: &[(String, String)]) {
    super::banner::render();
    super::help::render_all(layout, options);
}
