//! 命名空间的管理（增删改、别名、清空）。
//!
//! 从 `mod.rs` 搬出来：这一块自成一体，与阅读、写入、回收站那些逻辑没有交集。
//! 子模块可以访问父模块的私有项（字段、私有方法），所以这里是**纯粹的搬家**。

use super::*;

impl Vault {
    /// 命名空间表（设置页要看）
    pub fn namespaces(&self) -> Vec<Namespace> {
        self.table.items.clone()
    }

    /// 把命名空间表写回去。表现在可以被用户改，所以必须落盘。
    fn save_namespaces(&self) -> Result<(), VaultError> {
        write_atomic(
            &self.root.join("namespaces.json"),
            &serde_json::to_vec_pretty(&*self.table)?,
        )?;
        Ok(())
    }

    /// 新增一个命名空间。标识就是名字本身。
    pub fn add_namespace(
        &mut self,
        name: &str,
        aliases: Vec<String>,
        site: Option<String>,
    ) -> Result<(), VaultError> {
        let mut table = (*self.table).clone();
        table
            .add(name, aliases, site)
            .map_err(VaultError::BadAddress)?;
        self.table = Arc::new(table);
        self.save_namespaces()
    }

    /// 把"名字或别名"解析成**标识**。
    ///
    /// 两跳寻址的关键一步：外面（界面、命令、地址）用名字，里面（目录、键）用标识。
    fn namespace_id(&self, name_or_alias: &str) -> String {
        let needle = name_or_alias.trim();
        // 空与 "0" 都是主命名空间
        if needle.is_empty() {
            return crate::title::MAIN_NS.to_string();
        }
        self.table
            .lookup(needle)
            .map(|item| item.id.clone())
            .unwrap_or_else(|| needle.to_string())
    }

    /// 清空一个命名空间：里面的页面全部进回收站（可还原），命名空间本身留着。
    pub fn empty_namespace(&self, key: &str) -> Result<usize, VaultError> {
        // 传进来的可能是名字（界面按名字称呼它），这里换成标识再比 —— 目录用的是标识
        let id = self.namespace_id(key);
        let titles: Vec<String> = self
            .walk(&self.notes_dir())?
            .iter()
            .filter(|parsed| parsed.ns == id)
            .map(|parsed| parsed.display(&self.table))
            .collect();

        let mut count = 0;
        for title in titles {
            // 走既有的删除路径：写删除标记、文件搬进回收站 —— 一件事只实现一次
            self.delete(&title)?;
            count += 1;
        }
        Ok(count)
    }

    /// 改一个命名空间的别名（增、删、一次给多个都用它）。
    ///
    /// 名称与标识都不动 —— 别名只是"另外几个也能写的前缀"。保留的两个（主命名空间与
    /// `special`）**也可以有别名**：给主命名空间配 `主`，`[[主:某页]]` 就落在主命名空间。
    pub fn update_namespace_aliases(
        &mut self,
        key: &str,
        aliases: Vec<String>,
    ) -> Result<(), VaultError> {
        let id = self.namespace_id(key);
        let mut table = (*self.table).clone();
        let Some(item) = table.items.iter_mut().find(|item| item.id == id) else {
            return Err(VaultError::BadAddress(format!("命名空间「{key}」不存在")));
        };
        item.aliases = aliases;
        // 先校验、通过了才替换：重名与非法字符都在这里拦下
        table.validate().map_err(VaultError::BadAddress)?;
        self.table = Arc::new(table);
        self.save_namespaces()
    }

    /// 给命名空间改名。
    ///
    /// 两跳寻址下这**只是改表里一行**：磁盘目录与标题键用的都是标识，所以
    /// **一个文件都不用动**（这也正是当初把标识与名字分开的理由）。
    /// 需要跟着走的只有名字表里的**完整显示标题**前缀。
    pub fn rename_namespace(&mut self, key: &str, new_name: &str) -> Result<(), VaultError> {
        let id = self.namespace_id(key);
        if NamespaceTable::is_reserved(&id) {
            return Err(VaultError::BadAddress(format!(
                "「{key}」是保留的命名空间，不能改名"
            )));
        }
        let old_name = self
            .table
            .get(&id)
            .map(|item| item.name.clone())
            .ok_or_else(|| VaultError::BadAddress(format!("命名空间「{key}」不存在")))?;

        let new_name = new_name.trim().to_string();
        let mut table = (*self.table).clone();
        let Some(item) = table.items.iter_mut().find(|item| item.id == id) else {
            return Err(VaultError::BadAddress(format!("命名空间「{key}」不存在")));
        };
        item.name = new_name.clone();
        // 先校验、通过了才替换：失败不留半张表
        table.validate().map_err(VaultError::BadAddress)?;
        self.table = Arc::new(table);

        if !old_name.is_empty() && old_name != new_name {
            let mut titles = self.titles()?;
            let old_prefix = format!("{old_name}:");
            let new_prefix = format!("{new_name}:");
            for map in [&mut titles.notes, &mut titles.trashed] {
                for value in map.values_mut() {
                    if let Some(rest) = value.strip_prefix(&old_prefix) {
                        *value = format!("{new_prefix}{rest}");
                    }
                }
            }
            self.save_titles(&titles)?;
        }

        self.save_namespaces()
    }

    /// 删除一个命名空间：先清空（页面进回收站），再把自己从表里去掉。
    ///
    /// 键就是名字，所以**删掉再建同名 = 没删过**（你说的那条）。
    /// 主命名空间与 `special` 不可删。
    pub fn delete_namespace(&mut self, key: &str) -> Result<usize, VaultError> {
        let id = self.namespace_id(key);
        if NamespaceTable::is_reserved(&id) {
            return Err(VaultError::BadAddress(format!(
                "「{key}」是不可删除的命名空间"
            )));
        }
        let count = self.empty_namespace(&id)?;

        let mut table = (*self.table).clone();
        table.items.retain(|item| item.id != id);
        self.table = Arc::new(table);
        self.save_namespaces()?;
        Ok(count)
    }
}
