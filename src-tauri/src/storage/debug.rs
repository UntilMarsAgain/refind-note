//! 诊断报告：把"仓库现在是什么样"摊开成可读的分段。
//!
//! 它是一项**功能**（`special:debug`），不是调试残留：排查"模板取不到""页面去哪了"
//! "占了多少空间"这类问题时，这是唯一能一次看全的地方。因此这里只放**稳定的事实** ——
//! 路径、计数、表内容、解析结果；不放临时打印。
//!
//! 报告的每一段都对应界面上的一张小表；前端只负责显示与复制。
use super::api::{DebugEntry, DebugReport, DebugSection, RenderReport};
use super::{now_iso, Event, Vault};
use std::fs;

fn entry(label: impl Into<String>, value: impl Into<String>) -> DebugEntry {
    DebugEntry {
        label: label.into(),
        value: value.into(),
    }
}

fn section(title: impl Into<String>, entries: Vec<DebugEntry>) -> DebugSection {
    DebugSection {
        title: title.into(),
        entries,
    }
}

impl Vault {
    /// 生成诊断报告。`address` 是当前地址（可为空）：给了就多一段"当前页"。
    pub fn debug_report(&self, address: Option<&str>) -> DebugReport {
        let mut sections = vec![
            self.debug_environment(),
            self.debug_settings(),
            self.debug_namespaces(),
            self.debug_storage(),
        ];
        if let Some(address) = address.filter(|text| !text.trim().is_empty()) {
            sections.push(self.debug_address(address));
        }
        DebugReport { sections }
    }

    fn debug_environment(&self) -> DebugSection {
        section(
            "版本与环境",
            vec![
                entry("应用版本", env!("CARGO_PKG_VERSION")),
                entry(
                    "平台",
                    format!("{} / {}", std::env::consts::OS, std::env::consts::ARCH),
                ),
                entry("仓库目录", self.root.display().to_string()),
                entry("存储格式版本", self.config.format.to_string()),
                entry("报告时间", now_iso()),
            ],
        )
    }

    fn debug_settings(&self) -> DebugSection {
        let item = |label: &str, value: String| entry(label, value);
        section(
            "设置",
            vec![
                item(
                    "标题首字母大写（capital_links）",
                    self.config.capital_links.to_string(),
                ),
                item(
                    "标题字节上限（max_title_bytes）",
                    self.config.max_title_bytes.to_string(),
                ),
                item(
                    "增量链长度上限（delta_chain_limit）",
                    self.config.delta_chain_limit.to_string(),
                ),
                item(
                    "回收站保留天数（trash_keep_days）",
                    self.config.trash_keep_days.to_string(),
                ),
                item(
                    "自动回收间隔（gc_interval_days）",
                    self.config.gc_interval_days.to_string(),
                ),
                item("上次清理回收站", self.config.last_trash_purge.clone()),
                item("上次回收", self.config.last_gc.clone()),
                item(
                    "外观",
                    format!(
                        "主题 {} ｜ 主题色 {} ｜ 阅读宽度 {}px ｜ 缩放 {}",
                        self.preferences.theme,
                        self.preferences.accent,
                        self.preferences.reading_width,
                        self.preferences.zoom
                    ),
                ),
                item("有待执行的维护", self.maintenance_pending().to_string()),
            ],
        )
    }

    fn debug_namespaces(&self) -> DebugSection {
        let entries = self
            .namespaces()
            .into_iter()
            .map(|item| {
                let aliases = if item.aliases.is_empty() {
                    "无".to_string()
                } else {
                    item.aliases.join("、")
                };
                let kind = match (&item.site, item.storable) {
                    (Some(site), _) => format!("跨站 → {site}"),
                    (None, true) => "内容（本仓库）".to_string(),
                    (None, false) => "虚拟（程序提供）".to_string(),
                };
                let name = if item.name.is_empty() {
                    "（主命名空间）".to_string()
                } else {
                    item.name.clone()
                };
                entry(
                    format!("{name}（标识 {}）", item.id),
                    format!("别名：{aliases} ｜ {kind}"),
                )
            })
            .collect();
        section("命名空间", entries)
    }

    fn debug_storage(&self) -> DebugSection {
        let mut entries = Vec::new();

        // 一次遍历同时算总数与各命名空间的分布
        let pages = self.walk(&self.notes_dir()).unwrap_or_default();
        entries.push(entry("笔记总数", pages.len().to_string()));
        for item in self.namespaces() {
            let count = pages.iter().filter(|parsed| parsed.ns == item.id).count();
            let name = if item.name.is_empty() {
                "（主命名空间）".to_string()
            } else {
                item.name.clone()
            };
            entries.push(entry(format!("　{name}"), format!("{count} 篇")));
        }

        // 内容块库：正文按内容哈希去重存放，所以这里能看出"重复内容省了多少"
        let (mut blob_count, mut blob_bytes) = (0usize, 0u64);
        if let Ok(items) = fs::read_dir(self.root.join("blobs")) {
            for item in items.flatten() {
                if let Ok(meta) = item.metadata() {
                    if meta.is_file() {
                        blob_count += 1;
                        blob_bytes += meta.len();
                    }
                }
            }
        }
        entries.push(entry(
            "内容块",
            format!("{blob_count} 个，共 {} KiB", blob_bytes / 1024),
        ));

        match self.list_trash() {
            Ok(trash) => entries.push(entry("回收站", format!("{} 条", trash.len()))),
            Err(error) => entries.push(entry("回收站", format!("读不出来：{error}"))),
        }

        // 模板页：取用模板时看的就是这张表
        let templates = self.template_names();
        entries.push(entry(
            format!("模板页（template: 下）{} 个", templates.len()),
            if templates.is_empty() {
                "（一个都没有）".to_string()
            } else {
                templates
                    .iter()
                    .map(|(name, bytes)| format!("{name}（{bytes} 字节）"))
                    .collect::<Vec<_>>()
                    .join("、")
            },
        ));

        section("内容与体积", entries)
    }

    /// 当前地址：解析结果、这一页的规模与历史，以及**模板块会走哪条分发路径**
    fn debug_address(&self, address: &str) -> DebugSection {
        let mut entries = vec![entry("地址", address)];

        match self.parse_address(address) {
            Ok(parsed) => entries.push(entry("解析结果", format!("{parsed:?}"))),
            Err(error) => {
                entries.push(entry("解析结果", format!("解析失败：{error}")));
                return section("当前页", entries);
            }
        }

        let Ok(Some(markdown)) = self.current_markdown(address) else {
            entries.push(entry("正文", "（这一页还没有内容，或不是可读的页面）"));
            return section("当前页", entries);
        };

        entries.push(entry(
            "正文",
            format!("{} 行，{} 字节", markdown.lines().count(), markdown.len()),
        ));

        // 语言判定与前端编辑器用的同一条规则（`code_template_language`）
        if let Ok(parsed) = self.table.parse(address, self.config.capital_links) {
            let language =
                crate::title::code_template_language(&parsed.ns, &parsed.title).unwrap_or("markdown");
            entries.push(entry("按什么语言对待", language));
        }

        // 历史规模：一次提交一条 Rev，自动保存一条 Auto
        if let Ok((_, path)) = self.locate(address) {
            let events = self.read_events(&Self::id_from_path(&path)).unwrap_or_default();
            let commits = events
                .iter()
                .filter(|event| matches!(event, Event::Rev { .. }))
                .count();
            let drafts = events
                .iter()
                .filter(|event| matches!(event, Event::Auto { .. }))
                .count();
            entries.push(entry(
                "历史",
                format!("{commits} 次提交，{drafts} 份草稿（共 {} 条事件）", events.len()),
            ));
        }

        entries.extend(self.debug_template_probe(&markdown));
        section("当前页", entries)
    }

    /// 扫出正文里的模板块头，说明每一条会走哪条分发路径。
    ///
    /// 这是**近似**：真正的块识别（缩进划界）在 markdown 解析器里，这里只看行首的 `::`。
    /// 目的不是复刻解析器，而是回答"我写的这个模板为什么没生效"：
    /// 名字对不对、内建表里有没有、模板命名空间里有没有、`src=` 指的页面在不在。
    /// 编辑器预览的编译报告：把"这次渲染发生了什么"摊开。
    ///
    /// 与诊断页共用探针与语言判定 —— 同一件事只有一处说法，两边的结论不会分家。
    /// **按需调用**（面板上的「收集」），不跟着每次输入跑：预览本身走的是另一条命令。
    pub fn render_report(&self, markdown: &str, address: Option<&str>) -> RenderReport {
        let started = std::time::Instant::now();
        let html = self.render(markdown);
        let millis = started.elapsed().as_millis() as u64;

        let language = address
            .and_then(|address| self.table.parse(address, self.config.capital_links).ok())
            .and_then(|parsed| crate::title::code_template_language(&parsed.ns, &parsed.title))
            .unwrap_or("markdown")
            .to_string();

        RenderReport {
            markdown_bytes: markdown.len(),
            markdown_lines: markdown.lines().count(),
            html_bytes: html.len(),
            html,
            millis,
            language,
            blocks: self.debug_template_probe(markdown),
        }
    }

    pub(crate) fn debug_template_probe(&self, markdown: &str) -> Vec<DebugEntry> {
        let templates = self.template_names();
        let mut found: Vec<DebugEntry> = Vec::new();

        for line in markdown.lines() {
            let Some((name, params)) =
                crate::markdown::syntax::template::Template::parse_header(line.trim_start())
            else {
                continue;
            };

            let mut route = if crate::markdown::syntax::template::TEMPLATES
                .iter()
                .any(|(standard, _)| *standard == name)
            {
                "内建模板".to_string()
            } else if templates.iter().any(|(page, _)| *page == name) {
                format!("模板页 template:{name}")
            } else {
                "查不到 → 会渲染成「未知模板」框".to_string()
            };

            // `src=` 是"素材放在模板页里"的用法，最容易出问题，所以单独核对
            if let Some((_, source)) = params.iter().find(|(key, _)| key == "src") {
                let exists = templates.iter().any(|(page, _)| page == source);
                route.push_str(&format!(
                    "；src={source} → {}",
                    if exists { "找得到" } else { "找不到" }
                ));
            }

            let shown = if params.is_empty() {
                String::new()
            } else {
                format!(
                    "（{}）",
                    params
                        .iter()
                        .map(|(key, value)| format!("{key}={value}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            };
            found.push(entry(format!("::{name}{shown}"), route));
        }

        if found.is_empty() {
            found.push(entry("模板块", "正文里没有模板块"));
        }
        found
    }
}

