//! 媒体类：`image` / `video` / `audio` —— 把一个文件摆出来。
//!
//! 三者**共用同一套规矩**，所以必须住在一起，分开写就会各抄一份、迟早走样：
//!
//! - `src=` 必给，且协议只挡能执行或能外传数据的三种（`javascript:` / `data:` / `vbscript:`）；
//! - `width=` / `height=` 是**上限**（`max-width` / `max-height`），数字与单位都受限
//!   （见 [`size_rule`]），免得有人拿尺寸参数往 `style` 里塞别的东西；
//! - 图注两种写法：`caption="…"`，或者**在块里写一行**（最自然的写法）；块内容按行取、
//!   去掉缩进再拼成一句 —— 它在块里是被缩进的，不该带着空格显示。
//!
//! 区别只在最后落成什么标签：`::image` 是一张图（懒加载），`::video` / `::audio` 是播放器，
//! 所以后两者共用 [`render_media`]，只差一个标签名。
//!
//! `src` 写的是**仓库里的名字**（`片子.mp4`），与 `![]()` 里写的是同一个东西；
//! 换成能取到字节的地址是前端注入之后的事。

use super::super::dispatch::render_problem;
use super::super::parse::Template;
use super::size_rule;
use markdown_it::{Node, Renderer};

/// `::image src=…` —— 插入图片。`src` 必给；尺寸、位置、注释都可选：
///
/// - `align=left|center|right`（默认居中）；
/// - `width=` / `height=` —— 都是**上限**（`max-width` / `max-height`），图片不会被拉变形，
///   数量与单位都受限（见 [`size_rule`]），免得有人拿尺寸参数往 `style` 里塞别的东西；
/// - 注释有两种写法：**图片下面写一行**（最自然），或者 `caption="…"`。
///
/// 协议只挡能执行或能外传数据的三种：`javascript:` / `data:` / `vbscript:`。
pub(super) fn render_image(template: &Template, _node: &Node, fmt: &mut dyn Renderer) {
    let Some(source) = template.param("src") else {
        render_problem(template, fmt, "缺少 src=…：图片模板至少要给出图片地址");
        return;
    };
    let lowered = source.trim().to_lowercase();
    if ["javascript:", "data:", "vbscript:"]
        .iter()
        .any(|bad| lowered.starts_with(bad))
    {
        render_problem(
            template,
            fmt,
            "src 用的是不允许的协议（javascript / data / vbscript 会被拒绝）",
        );
        return;
    }

    let align = match template.param("align").map(str::trim) {
        Some("left") => "left",
        Some("right") => "right",
        _ => "center",
    };

    let mut style = String::new();
    if let Some(rule) = template
        .param("width")
        .and_then(|value| size_rule("max-width", value))
    {
        style.push_str(&rule);
    }
    if let Some(rule) = template
        .param("height")
        .and_then(|value| size_rule("max-height", value))
    {
        style.push_str(&rule);
    }

    fmt.cr();
    fmt.open("figure", &[("class", format!("image image--{align}"))]);
    fmt.cr();
    let mut attrs: Vec<(&str, String)> = vec![
        ("src", source.trim().to_string()),
        (
            "alt",
            template.param("alt").unwrap_or("").trim().to_string(),
        ),
        // 长文里的图片不该抢首屏带宽
        ("loading", "lazy".to_string()),
    ];
    if !style.is_empty() {
        attrs.push(("style", style));
    }
    fmt.self_close("img", &attrs);
    fmt.cr();
    // 注释：块里的那一行（最自然的写法）优先用 `caption=` 覆盖。
    // 块内容按行取、去掉缩进再拼成一句：它在块里是被缩进的，不该带着空格显示。
    let caption = match template.param("caption") {
        Some(caption) => caption.trim().to_string(),
        None => template
            .body
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
    };
    if !caption.is_empty() {
        fmt.open("figcaption", &[]);
        fmt.text(&caption);
        fmt.close("figcaption");
        fmt.cr();
    }
    fmt.close("figure");
    fmt.cr();
}

/// `::video src=片子.mp4` / `::audio src=录音.mp3` —— 摆一个播放器。
///
/// 与 `::image` 同一套参数：`src` 必给，`width=` / `height=` 是**上限**，
/// 注释写在块里一行或由 `caption=` 给出。`align=` 只对视频有意义（音频是个窄条）。
pub(super) fn render_video(template: &Template, node: &Node, fmt: &mut dyn Renderer) {
    render_media(template, node, fmt, "video");
}

/// 见 [`render_video`]
pub(super) fn render_audio(template: &Template, node: &Node, fmt: &mut dyn Renderer) {
    render_media(template, node, fmt, "audio");
}

/// 视频与音频只差一个标签名：源、尺寸、注释、位置的规矩完全一样
fn render_media(template: &Template, _node: &Node, fmt: &mut dyn Renderer, kind: &str) {
    let Some(source) = template.param("src") else {
        render_problem(template, fmt, "缺少 src=…：至少要给出文件名字");
        return;
    };
    let lowered = source.trim().to_lowercase();
    if ["javascript:", "data:", "vbscript:"]
        .iter()
        .any(|bad| lowered.starts_with(bad))
    {
        render_problem(
            template,
            fmt,
            "src 用的是不允许的协议（javascript / data / vbscript 会被拒绝）",
        );
        return;
    }

    let align = match template.param("align").map(str::trim) {
        Some("left") => "left",
        Some("right") => "right",
        _ => "center",
    };

    let mut style = String::new();
    if let Some(rule) = template
        .param("width")
        .and_then(|value| size_rule("max-width", value))
    {
        style.push_str(&rule);
    }
    if let Some(rule) = template
        .param("height")
        .and_then(|value| size_rule("max-height", value))
    {
        style.push_str(&rule);
    }

    fmt.cr();
    fmt.open(
        "figure",
        &[("class", format!("media media--{kind} media--{align}"))],
    );
    fmt.cr();
    let mut attrs: Vec<(&str, String)> = vec![
        ("src", source.trim().to_string()),
        // 播放器该有的控件一个不少；预加载只取头，别让长片子一打开就拖满带宽
        ("controls", "controls".to_string()),
        ("preload", "metadata".to_string()),
    ];
    if !style.is_empty() {
        attrs.push(("style", style));
    }
    fmt.self_close(kind, &attrs);
    fmt.cr();
    if let Some(caption) = caption_of(template) {
        fmt.open("figcaption", &[]);
        fmt.text(&caption);
        fmt.close("figcaption");
        fmt.cr();
    }
    fmt.close("figure");
    fmt.cr();
}

/// 注释：`caption=` 优先，其次块里写着的那一行（缩进与空行都不算）
fn caption_of(template: &Template) -> Option<String> {
    match template.param("caption") {
        Some(caption) => Some(caption.trim().to_string()).filter(|text| !text.is_empty()),
        None => {
            let text = template
                .body
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            Some(text).filter(|text| !text.is_empty())
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::markdown::render;

    /// `::video` / `::audio`：摆一个带控件的播放器（源写的是仓库里的名字）
    #[test]
    fn video_and_audio_render_players() {
        let video = render("::video src=片子.mp4 width=720px\n  一段注释\n");
        assert!(video.contains("<video"), "{video}");
        assert!(video.contains("controls"), "{video}");
        assert!(video.contains("src=\"片子.mp4\""), "{video}");
        assert!(video.contains("max-width: 720px"), "{video}");
        assert!(
            video.contains("<figcaption>一段注释</figcaption>"),
            "{video}"
        );

        let audio = render("::audio src=录音.mp3\n");
        assert!(audio.contains("<audio"), "{audio}");
        assert!(audio.contains("controls"), "{audio}");

        // 没有 src 就报用法问题，而不是摆一个空的播放器
        let bare = render("::video\n");
        assert!(bare.contains("template--problem"), "{bare}");
    }

    #[test]
    fn image_carries_alignment_and_limits() {
        let html = render("::image src=/logo.svg align=right width=320 caption=\"桥体\"\n");
        assert!(
            html.contains(r#"<figure class="image image--right">"#),
            "{html}"
        );
        assert!(html.contains(r#"src="/logo.svg""#), "{html}");
        assert!(html.contains("max-width: 320px"), "{html}");
        assert!(html.contains("<figcaption>桥体</figcaption>"), "{html}");
    }

    #[test]
    fn image_takes_its_caption_from_the_body() {
        // 图片下面写一行 —— 最自然的写法
        let html = render("::image src=/logo.svg\n  桥体（一〇七九年）\n");
        assert!(
            html.contains("<figcaption>桥体（一〇七九年）</figcaption>"),
            "{html}"
        );

        // 块内容在块里是被缩进的，注释不该带着那些空格
        let indented = render("::image src=/logo.svg\n    两边都有空格\n");
        assert!(
            indented.contains("<figcaption>两边都有空格</figcaption>"),
            "{indented}"
        );

        // 多行拼成一句
        let multiline = render("::image src=/logo.svg\n  第一行\n  第二行\n");
        assert!(
            multiline.contains("<figcaption>第一行 第二行</figcaption>"),
            "{multiline}"
        );

        // `caption=` 优先于块内容
        let explicit = render("::image src=/logo.svg caption=显式\n  块里那句\n");
        assert!(
            explicit.contains("<figcaption>显式</figcaption>"),
            "{explicit}"
        );
        assert!(!explicit.contains("块里那句"), "{explicit}");

        // 都没有：不出现空的图注
        let none = render("::image src=/logo.svg\n");
        assert!(!none.contains("<figcaption>"), "{none}");
    }

    #[test]
    fn image_refuses_dangerous_sources_and_sneaky_sizes() {
        // 危险协议：不渲染图片，只给提示
        // （提示框里会**回显**参数原文，那是文本、不是属性 —— 所以断言针对"有没有 img"）
        let bad = render("::image src=javascript:alert(1)\n");
        assert!(!bad.contains("<img"), "危险协议不该渲染成图片：{bad}");
        assert!(bad.contains("template--problem"), "{bad}");

        // 尺寸只认"数字 + 可选单位"：塞别的声明一律不认，因此拼不出 style 属性
        let sneaky = render("::image src=/x.svg width=\"1px; background: url(//evil)\"\n");
        assert!(
            !sneaky.contains("style="),
            "尺寸参数不该拼出 style 属性：{sneaky}"
        );

        // 没有 src：说清缺什么
        let missing = render("::image\n");
        assert!(missing.contains("template--problem"), "{missing}");
        assert!(missing.contains("src"), "{missing}");
    }
}
