fn main() {
    // `help/` 整个目录由 `include_dir!` 在编译时嵌进程序。**新加一个文件**时
    // 宏不会自己发现（它只记住当时有哪些文件），所以这里让 cargo 盯着这个目录：
    // 往里丢一个 .md 就会重新编译，帮助页随即出现在界面上。
    println!("cargo:rerun-if-changed=help");
    tauri_build::build()
}
