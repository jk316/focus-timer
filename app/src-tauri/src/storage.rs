//! 会话持久化：本地 JSON 文件 + 按日累计。
//!
//! 对应 SPEC：FR-4（每日累计专注时长、按日翻看历史、重启不丢失）。
//! 记录归属日期 = 会话开始时间的本地日期（见 [`Session::date`]）。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    /// 归属日期（YYYY-MM-DD，本地时区，取开始时间）。
    pub date: String,
    /// 开始时间（本地 ISO 格式）。
    pub start_at: String,
    /// 结束时间（本地 ISO 格式）。
    pub end_at: String,
    /// 实际专注秒数。
    pub duration_secs: u64,
    /// 任务描述。
    pub task: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SessionsFile {
    sessions: Vec<Session>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum StoreError {
    Io(String),
    Serialize(String),
}

#[derive(Debug)]
pub struct Store {
    path: PathBuf,
    sessions: Vec<Session>,
}

impl Store {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            sessions: Vec::new(),
        }
    }

    /// 从文件加载；文件不存在时返回空 Store。
    pub fn load(path: PathBuf) -> Result<Self, StoreError> {
        let mut store = Self::new(path);
        if store.path.exists() {
            let text = fs::read_to_string(&store.path).map_err(|e| StoreError::Io(e.to_string()))?;
            let file: SessionsFile =
                serde_json::from_str(&text).map_err(|e| StoreError::Serialize(e.to_string()))?;
            store.sessions = file.sessions;
        }
        Ok(store)
    }

    pub fn save(&self) -> Result<(), StoreError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| StoreError::Io(e.to_string()))?;
        }
        let file = SessionsFile {
            sessions: self.sessions.clone(),
        };
        let text =
            serde_json::to_string_pretty(&file).map_err(|e| StoreError::Serialize(e.to_string()))?;
        fs::write(&self.path, text).map_err(|e| StoreError::Io(e.to_string()))
    }

    pub fn add(&mut self, session: Session) -> Result<(), StoreError> {
        self.sessions.push(session);
        self.save()
    }

    pub fn sessions_on(&self, date: &str) -> Vec<&Session> {
        let mut v: Vec<&Session> = self.sessions.iter().filter(|s| s.date == date).collect();
        // 倒序：时间最新的排最上面（start_at 为 ISO 格式，字典序即时间序）
        v.sort_by(|a, b| b.start_at.cmp(&a.start_at));
        v
    }

    pub fn total_seconds_on(&self, date: &str) -> u64 {
        self.sessions_on(date).iter().map(|s| s.duration_secs).sum()
    }

    /// 所有出现过的日期，按降序（最新在前）去重。
    pub fn all_dates(&self) -> Vec<String> {
        let mut dates: Vec<String> = self.sessions.iter().map(|s| s.date.clone()).collect();
        dates.sort();
        dates.dedup();
        dates.reverse();
        dates
    }

    pub fn sessions(&self) -> &[Session] {
        &self.sessions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "focus-timer-test-{}-{}.json",
            name,
            std::process::id()
        ))
    }

    fn sample(id: &str, date: &str, secs: u64, task: &str) -> Session {
        Session {
            id: id.into(),
            date: date.into(),
            start_at: format!("{date}T09:00:00"),
            end_at: format!("{date}T09:30:00"),
            duration_secs: secs,
            task: task.into(),
        }
    }

    #[test]
    fn add_and_reload_persists() {
        let path = tmp_path("persist");
        let _ = std::fs::remove_file(&path);
        {
            let mut store = Store::new(path.clone());
            store.add(sample("1", "2026-01-01", 1800, "写代码")).unwrap();
        }
        let store = Store::load(path.clone()).unwrap();
        assert_eq!(store.sessions().len(), 1);
        assert_eq!(store.sessions()[0].task, "写代码");
        assert_eq!(store.sessions()[0].duration_secs, 1800);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let path = tmp_path("missing");
        let _ = std::fs::remove_file(&path);
        let store = Store::load(path).unwrap();
        assert!(store.sessions().is_empty());
    }

    #[test]
    fn total_seconds_groups_by_date() {
        let path = tmp_path("total");
        let _ = std::fs::remove_file(&path);
        let mut store = Store::new(path.clone());
        store.add(sample("1", "2026-01-01", 1800, "a")).unwrap();
        store.add(sample("2", "2026-01-01", 900, "b")).unwrap();
        store.add(sample("3", "2026-01-02", 600, "c")).unwrap();
        assert_eq!(store.total_seconds_on("2026-01-01"), 2700);
        assert_eq!(store.total_seconds_on("2026-01-02"), 600);
        assert_eq!(store.total_seconds_on("2026-01-03"), 0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn all_dates_sorted_desc_unique() {
        let path = tmp_path("dates");
        let _ = std::fs::remove_file(&path);
        let mut store = Store::new(path.clone());
        store.add(sample("1", "2026-01-01", 100, "a")).unwrap();
        store.add(sample("2", "2026-01-03", 100, "b")).unwrap();
        store.add(sample("3", "2026-01-01", 100, "c")).unwrap();
        assert_eq!(
            store.all_dates(),
            vec!["2026-01-03".to_string(), "2026-01-01".to_string()]
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn sessions_on_returns_newest_first() {
        let path = tmp_path("order");
        let _ = std::fs::remove_file(&path);
        let mut store = Store::new(path.clone());
        let mut s1 = sample("1", "2026-01-01", 100, "早");
        s1.start_at = "2026-01-01T09:00:00".into();
        let mut s2 = sample("2", "2026-01-01", 200, "中");
        s2.start_at = "2026-01-01T11:00:00".into();
        let mut s3 = sample("3", "2026-01-01", 300, "晚");
        s3.start_at = "2026-01-01T15:00:00".into();
        store.add(s1).unwrap();
        store.add(s2).unwrap();
        store.add(s3).unwrap();
        let ss = store.sessions_on("2026-01-01");
        let tasks: Vec<&str> = ss.iter().map(|s| s.task.as_str()).collect();
        assert_eq!(tasks, vec!["晚", "中", "早"]);
        let _ = std::fs::remove_file(&path);
    }
}
