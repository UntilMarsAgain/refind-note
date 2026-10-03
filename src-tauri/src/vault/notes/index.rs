//! 名字表与定位：标题 ↔ id，以及这篇笔记的文件落在哪。
//!
//! 一切"这篇是谁"的判断都从这里出发 —— 磁盘上只有生成的 ASCII id，中文标题只活在
//! `titles.json` 里，所以**改名不需要动任何文件**。

use std::fs;
use std::path::PathBuf;


use super::{fold, Event, NoteState, Titles};
use crate::storage::workspace::{append_line, read_json, write_json};
use crate::vault::database::Database;
use crate::vault::namespace::MAIN_ID;

impl Database {
    pub fn titles(&self) -> Result<Titles, String> {
        Ok(read_json::<Titles>(&self.titles_path()))
    }

    pub fn save_titles(&self, titles: &Titles) -> Result<(), String> {
        write_json(&self.titles_path(), titles)
    }

    /// 把地址里写的东西规整成**显示标题**。
    ///
    /// 创建、查表、地址解析都过这一道 —— 同一把尺子，所以 `example` 与 `Example`
    /// 是同一篇；前缀写别名也认，但显示出来的是规范名。
    pub fn display_of(&self, title: &str) -> Result<String, String> {
        let table = self.namespaces();
        let parsed = crate::vault::title::parse(title, &table)?;
        Ok(parsed.display(&table))
    }

    /// 标题 → 命名空间 + 页面名（创建、查表、渲染都从这里过）
    pub fn parse_title(&self, title: &str) -> Result<crate::vault::title::ParsedTitle, String> {
        crate::vault::title::parse(title, &self.namespaces())
    }

    /// 这篇笔记在哪个命名空间里。
    ///
    /// 标识本身不含命名空间，只能从显示标题看出来。查不到就按主命名空间算 ——
    /// 表被改过、或笔记是从别处搬来的，都不该让整篇读不出来。
    fn ns_of_id(&self, id: &str) -> String {
        let Ok(titles) = self.titles() else {
            return MAIN_ID.to_string();
        };
        let Some(display) = titles.notes.get(id).or_else(|| titles.trashed.get(id)) else {
            return MAIN_ID.to_string();
        };
        match crate::vault::title::parse(display, &self.namespaces()) {
            Ok(parsed) => parsed.ns,
            Err(_) => MAIN_ID.to_string(),
        }
    }

    /// 一篇笔记的日志路径
    pub fn log_path(&self, id: &str) -> PathBuf {
        self.note_path(&self.ns_of_id(id), id)
    }

    /// 已删除笔记的日志路径
    pub fn trash_path(&self, id: &str) -> PathBuf {
        self.trash_note_path(&self.ns_of_id(id), id)
    }

    /// 标题 → id
    ///
    /// 查表前先把名字规整成显示标题：命令也可能被直接调用（前端、脚本），
    /// 与地址解析用同一把尺子才不会出现"建了 example、敲 Example 找不到"。
    pub fn id_of(&self, title: &str) -> Option<String> {
        let table = self.namespaces();
        // 前缀写的是别名也认：比的是**解析出来的**（命名空间, 页面名），不是字面
        let wanted = crate::vault::title::parse(title, &table).ok()?;

        self.titles()
            .ok()?
            .notes
            .iter()
            .find(|(_, display)| {
                crate::vault::title::parse(display, &table)
                    .map(|parsed| parsed.ns == wanted.ns && parsed.page == wanted.page)
                    .unwrap_or(false)
            })
            .map(|(id, _)| id.clone())
    }

    /// 标题在不在
    pub fn exists(&self, title: &str) -> bool {
        self.id_of(title).is_some()
    }

    /// 标题 → id。找不到就是"没有这篇"。
    pub(crate) fn locate(&self, title: &str) -> Result<String, String> {
        self.id_of(title)
            .ok_or_else(|| format!("没有这篇笔记：{title}"))
    }

    pub(crate) fn read_events(&self, id: &str) -> Result<Vec<Event>, String> {
        let path = self.log_path(id);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            // 日志不在 = 这篇没有历史（刚建、或被人删了文件），不是错误
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(format!("读不出 {}：{error}", path.display())),
        };

        let mut events = Vec::new();
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            events.push(
                serde_json::from_str(line)
                    .map_err(|error| format!("{} 里有一行读不出来：{error}", path.display()))?,
            );
        }
        Ok(events)
    }

    pub(crate) fn append(&self, id: &str, event: &Event) -> Result<(), String> {
        let line = serde_json::to_string(event).map_err(|error| format!("写不进日志：{error}"))?;
        append_line(&self.log_path(id), &line)
    }

    pub(crate) fn state_of(&self, id: &str) -> Result<NoteState, String> {
        Ok(fold(&self.read_events(id)?))
    }
}
