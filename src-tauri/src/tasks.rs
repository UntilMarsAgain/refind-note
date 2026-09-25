//! 异步任务。
//!
//! 长期操作（数据库整理，以后的网络同步）不该让界面卡在按钮上等：点一下**只提交任务**，
//! 由后台线程去做，界面看状态即可。
//!
//! 任务表放在**进程内**、不落盘：它描述的是"这次运行正在做什么"，不是数据。重启后那些
//! 操作要么已完成、要么根本没开始，没有需要恢复的东西。
//!
//! 与写锁的关系：`job` 在后台线程里执行，**写锁由调用方在 job 内部取** —— 这样后台任务
//! 与前台命令之间仍然是串行写，两者不会互相踩。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::Serialize;

/// 任务状态
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskState {
    Queued,
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub id: u64,
    /// 任务名：界面上直接显示，所以写成人话
    pub label: String,
    pub state: TaskState,
    pub submitted_at: String,
    /// 结束时才写；未结束时为空
    pub finished_at: String,
    /// 结果说明：成功写做了什么，失败写原因
    pub message: String,
}

/// 表里最多留多少条历史，免得无限长
const MAX_KEPT: usize = 20;

static TASKS: OnceLock<Mutex<Vec<Task>>> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn tasks() -> &'static Mutex<Vec<Task>> {
    TASKS.get_or_init(|| Mutex::new(Vec::new()))
}

fn stamp() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

pub fn list() -> Vec<Task> {
    tasks().lock().map(|list| list.clone()).unwrap_or_default()
}

fn update(id: u64, state: TaskState, message: String, finished: bool) {
    if let Ok(mut list) = tasks().lock() {
        if let Some(task) = list.iter_mut().find(|task| task.id == id) {
            task.state = state;
            task.message = message;
            if finished {
                task.finished_at = stamp();
            }
        }
    }
}

/// 提交一个任务：**立刻返回任务 id**，活儿交给后台线程。
pub fn submit<F>(label: impl Into<String>, job: F) -> u64
where
    F: FnOnce() -> Result<String, String> + Send + 'static,
{
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);

    if let Ok(mut list) = tasks().lock() {
        list.push(Task {
            id,
            label: label.into(),
            state: TaskState::Queued,
            submitted_at: stamp(),
            finished_at: String::new(),
            message: String::new(),
        });
        if list.len() > MAX_KEPT {
            let drop_count = list.len() - MAX_KEPT;
            list.drain(0..drop_count);
        }
    }

    std::thread::spawn(move || {
        update(id, TaskState::Running, String::new(), false);
        match job() {
            Ok(message) => update(id, TaskState::Done, message, true),
            Err(error) => update(id, TaskState::Failed, error, true),
        }
    });

    id
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 后台线程跑完会把状态与说明写回
    #[test]
    fn submitted_task_finishes() {
        let id = submit("测试任务", || Ok("做完了".to_string()));

        // 给它 2 秒；一旦完成就立刻返回，不白等
        for _ in 0..200 {
            let task = list()
                .into_iter()
                .find(|task| task.id == id)
                .expect("任务应当在表里");
            if task.state == TaskState::Done {
                assert_eq!(task.message, "做完了");
                assert!(!task.finished_at.is_empty(), "结束时应当写下时间");
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("任务在 2 秒内没有跑完");
    }

    /// 失败也要如实记下来，而不是留在"运行中"
    #[test]
    fn failed_task_records_the_reason() {
        let id = submit("会失败的任务", || Err("出错了".to_string()));

        for _ in 0..200 {
            let task = list()
                .into_iter()
                .find(|task| task.id == id)
                .expect("任务应当在表里");
            if task.state == TaskState::Failed {
                assert_eq!(task.message, "出错了");
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("任务在 2 秒内没有失败");
    }
}
