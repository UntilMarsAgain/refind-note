//! 这张表在**仓库上**怎么落。
//!
//! 这里是唯一碰磁盘的那一半：改表之后要写 `db/namespaces.json`、补建命名空间目录，
//! 改名时还得把标题里写下来的旧前缀一起换成新的。表内的约束（重名、保留名）不归这里管 ——
//! 那是 [`super::table`] 的事。
//!
//! 两个自由函数是这个模块的私有帮手：`namespace_id` 把命令传来的 `key`（标识、规范名或
//! 别名）统一换算成标识，`rename_in_titles` 负责批量改显示标题。

use super::{Namespace, NamespaceTable};
use crate::vault::database::Database;

impl Database {
    /// 整张表（命令直接把它交给界面）
    pub fn namespace_list(&self) -> Vec<Namespace> {
        self.namespaces().items
    }

    /// 新建一个命名空间，返回更新后的整张表。
    ///
    /// 表先落地、目录再补建：反过来的话，建目录成功而写表失败就会留下一个
    /// "有目录、没人认识"的命名空间。
    pub fn add_namespace(
        &self,
        name: &str,
        aliases: Vec<String>,
        site: Option<String>,
    ) -> Result<Vec<Namespace>, String> {
        let mut table = self.namespaces();
        table.add(name, aliases, site)?;
        self.save_namespaces(&table)?;
        self.ensure_namespace_dirs()?;
        Ok(table.items)
    }

    /// 改别名与站点地址
    pub fn update_namespace(
        &self,
        id: &str,
        aliases: Vec<String>,
        site: Option<String>,
    ) -> Result<Vec<Namespace>, String> {
        let mut table = self.namespaces();
        table.update(&namespace_id(&table, id), aliases, site)?;
        self.save_namespaces(&table)?;
        Ok(table.items)
    }

    /// 改名。文件一个不挪，只把两处名字改掉：表里那一行，和标题里写下来的前缀。
    pub fn rename_namespace(&self, id: &str, name: &str) -> Result<Vec<Namespace>, String> {
        let mut table = self.namespaces();
        let id = namespace_id(&table, id);
        let old = table.name_of(&id);
        table.rename(&id, name)?;
        let new = table.name_of(&id);
        self.save_namespaces(&table)?;

        // 显示标题里存着旧前缀：不加这一道，那些笔记就会"表里改了、标题里还写着旧的"
        let mut titles = self.titles()?;
        rename_in_titles(&mut titles.notes, &old, &new);
        rename_in_titles(&mut titles.trashed, &old, &new);
        self.save_titles(&titles)?;

        Ok(table.items)
    }

    /// 清空：把里面的页面**全部移进回收站**（不是抹掉），命名空间本身留着
    pub fn empty_namespace(&self, id: &str) -> Result<usize, String> {
        let table = self.namespaces();
        let id = namespace_id(&table, id);
        if table.get(&id).is_none() {
            return Err(format!("命名空间「{id}」不存在"));
        }

        // 先列出它名下的笔记（显示标题），再一个个走正常的删除路径
        let titles = self.titles()?;
        let mut doomed: Vec<String> = Vec::new();
        for display in titles.notes.values() {
            if let Ok(parsed) = crate::vault::title::parse(display, &table) {
                if parsed.ns == id {
                    doomed.push(display.clone());
                }
            }
        }

        for title in &doomed {
            self.delete(title)?;
        }
        Ok(doomed.len())
    }

    /// 删除命名空间：先清空（页面进回收站），再从表里去掉那一行
    pub fn delete_namespace(&self, id: &str) -> Result<usize, String> {
        let mut table = self.namespaces();
        let id = namespace_id(&table, id);
        let removed = self.empty_namespace(&id)?;
        table.remove(&id)?;
        self.save_namespaces(&table)?;
        Ok(removed)
    }
}

/// 命令里的 `key` 可以是标识、规范名或别名 —— 统一换算成标识
fn namespace_id(table: &NamespaceTable, key: &str) -> String {
    if table.get(key).is_some() {
        return key.to_string();
    }
    table
        .lookup(key)
        .map(|item| item.id.clone())
        .unwrap_or_else(|| key.to_string())
}

/// 把标题表里写着旧前缀的显示标题换成新前缀
fn rename_in_titles(titles: &mut std::collections::BTreeMap<String, String>, old: &str, new: &str) {
    for display in titles.values_mut() {
        let Some((prefix, page)) = display.split_once(':') else {
            continue;
        };
        if prefix != old {
            continue;
        }
        // 主命名空间没有前缀，改回它时要把冒号也去掉
        *display = if new.is_empty() {
            page.to_string()
        } else {
            format!("{new}:{page}")
        };
    }
}
