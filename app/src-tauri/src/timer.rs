//! 计时状态机（纯逻辑，可单测）。
//!
//! 对应 SPEC：FR-2（倒计时：开始/暂停/继续/重置/提前结束、归零不自动下一轮）、
//! FR-3（任务必填）。本模块不依赖真实时钟，时间由调用方通过 [`Timer::elapse`] 注入，
//! 因此可确定性单测。

/// 默认专注时长：30 分钟（秒）。
pub const DEFAULT_DURATION_SECS: u64 = 30 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Running,
    Paused,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerError {
    /// 任务为空（FR-3：任务必填）。
    EmptyTask,
    /// 时长非法（必须 > 0）。
    InvalidDuration,
    /// 当前状态不允许该操作。
    InvalidState,
}

/// 一次完成的专注会话（供上层持久化）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedSession {
    /// 实际专注秒数。
    pub duration_secs: u64,
    /// 任务描述。
    pub task: String,
    /// 是否自然归零结束（false = 手动提前结束）。
    pub finished_naturally: bool,
}

#[derive(Debug, Clone)]
pub struct Timer {
    phase: Phase,
    total_secs: u64,
    remaining_secs: u64,
    finished_naturally: bool,
    task: Option<String>,
}

impl Timer {
    pub fn new() -> Self {
        Self {
            phase: Phase::Idle,
            total_secs: 0,
            remaining_secs: 0,
            finished_naturally: false,
            task: None,
        }
    }

    /// 开始一个新的专注（FR-2 / FR-3：任务必填、时长 > 0）。
    pub fn start(&mut self, task: String, duration_secs: u64) -> Result<(), TimerError> {
        if task.trim().is_empty() {
            return Err(TimerError::EmptyTask);
        }
        if duration_secs == 0 {
            return Err(TimerError::InvalidDuration);
        }
        if self.phase == Phase::Running || self.phase == Phase::Paused {
            return Err(TimerError::InvalidState);
        }
        self.phase = Phase::Running;
        self.total_secs = duration_secs;
        self.remaining_secs = duration_secs;
        self.finished_naturally = false;
        self.task = Some(task);
        Ok(())
    }

    /// 暂停（仅运行中有效）。
    pub fn pause(&mut self) -> Result<(), TimerError> {
        if self.phase != Phase::Running {
            return Err(TimerError::InvalidState);
        }
        self.phase = Phase::Paused;
        Ok(())
    }

    /// 继续（仅暂停中有效）。
    pub fn resume(&mut self) -> Result<(), TimerError> {
        if self.phase != Phase::Paused {
            return Err(TimerError::InvalidState);
        }
        self.phase = Phase::Running;
        Ok(())
    }

    /// 重置（回到空闲，清除任务）。
    pub fn reset(&mut self) {
        self.phase = Phase::Idle;
        self.total_secs = 0;
        self.remaining_secs = 0;
        self.finished_naturally = false;
        self.task = None;
    }

    /// 时间流逝 `secs` 秒（仅运行中有效）。
    ///
    /// 返回 `true` 表示本次流逝导致自然归零（进入 Finished）；否则返回 `false`。
    /// 归零后不自动开始下一轮（FR-2）。
    pub fn elapse(&mut self, secs: u64) -> Result<bool, TimerError> {
        if self.phase != Phase::Running {
            return Err(TimerError::InvalidState);
        }
        if self.remaining_secs <= secs {
            self.remaining_secs = 0;
            self.phase = Phase::Finished;
            self.finished_naturally = true;
            Ok(true)
        } else {
            self.remaining_secs -= secs;
            Ok(false)
        }
    }

    /// 提前结束（运行中或暂停中有效）。进入 Finished 并标记为手动结束。
    pub fn finish(&mut self) -> Result<(), TimerError> {
        if self.phase != Phase::Running && self.phase != Phase::Paused {
            return Err(TimerError::InvalidState);
        }
        self.phase = Phase::Finished;
        self.finished_naturally = false;
        Ok(())
    }

    /// 取出完成结果并重置为 Idle。仅在 Finished 状态返回 Some。
    pub fn take_completed_session(&mut self) -> Option<CompletedSession> {
        if self.phase != Phase::Finished {
            return None;
        }
        let session = CompletedSession {
            duration_secs: self.total_secs - self.remaining_secs,
            task: self.task.clone().unwrap_or_default(),
            finished_naturally: self.finished_naturally,
        };
        self.reset();
        Some(session)
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn remaining_secs(&self) -> u64 {
        self.remaining_secs
    }
    pub fn total_secs(&self) -> u64 {
        self.total_secs
    }
    pub fn task(&self) -> Option<&str> {
        self.task.as_deref()
    }
}

impl Default for Timer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_duration_is_30_minutes() {
        assert_eq!(DEFAULT_DURATION_SECS, 1800);
    }

    #[test]
    fn start_requires_non_empty_task() {
        let mut t = Timer::new();
        assert_eq!(t.start("   ".into(), 1800), Err(TimerError::EmptyTask));
        assert_eq!(t.start(String::new(), 1800), Err(TimerError::EmptyTask));
    }

    #[test]
    fn start_requires_positive_duration() {
        let mut t = Timer::new();
        assert_eq!(t.start("写代码".into(), 0), Err(TimerError::InvalidDuration));
    }

    #[test]
    fn start_ok_sets_running_and_remaining() {
        let mut t = Timer::new();
        t.start("写代码".into(), 1800).unwrap();
        assert_eq!(t.phase(), Phase::Running);
        assert_eq!(t.remaining_secs(), 1800);
        assert_eq!(t.total_secs(), 1800);
        assert_eq!(t.task(), Some("写代码"));
    }

    #[test]
    fn cannot_start_while_running_or_paused() {
        let mut t = Timer::new();
        t.start("A".into(), 1800).unwrap();
        assert_eq!(t.start("B".into(), 1800), Err(TimerError::InvalidState));
        t.pause().unwrap();
        assert_eq!(t.start("B".into(), 1800), Err(TimerError::InvalidState));
    }

    #[test]
    fn elapse_decrements_remaining() {
        let mut t = Timer::new();
        t.start("A".into(), 1800).unwrap();
        assert_eq!(t.elapse(10).unwrap(), false);
        assert_eq!(t.remaining_secs(), 1790);
    }

    #[test]
    fn elapse_to_zero_finishes_naturally() {
        let mut t = Timer::new();
        t.start("A".into(), 100).unwrap();
        assert_eq!(t.elapse(100).unwrap(), true);
        assert_eq!(t.phase(), Phase::Finished);
        assert_eq!(t.remaining_secs(), 0);
        let s = t.take_completed_session().unwrap();
        assert_eq!(s.duration_secs, 100);
        assert_eq!(s.finished_naturally, true);
        assert_eq!(t.phase(), Phase::Idle);
    }

    #[test]
    fn elapse_past_zero_stays_finished_at_zero() {
        let mut t = Timer::new();
        t.start("A".into(), 100).unwrap();
        t.elapse(50).unwrap();
        assert_eq!(t.elapse(60).unwrap(), true);
        assert_eq!(t.remaining_secs(), 0);
        assert_eq!(t.phase(), Phase::Finished);
    }

    #[test]
    fn pause_and_resume() {
        let mut t = Timer::new();
        t.start("A".into(), 1800).unwrap();
        t.elapse(10).unwrap();
        t.pause().unwrap();
        assert_eq!(t.phase(), Phase::Paused);
        assert_eq!(t.elapse(10), Err(TimerError::InvalidState));
        assert_eq!(t.remaining_secs(), 1790);
        t.resume().unwrap();
        assert_eq!(t.phase(), Phase::Running);
        t.elapse(5).unwrap();
        assert_eq!(t.remaining_secs(), 1785);
    }

    #[test]
    fn pause_resume_invalid_states() {
        let mut t = Timer::new();
        assert_eq!(t.pause(), Err(TimerError::InvalidState));
        assert_eq!(t.resume(), Err(TimerError::InvalidState));
    }

    #[test]
    fn finish_early_computes_actual_duration() {
        let mut t = Timer::new();
        t.start("写周报".into(), 1800).unwrap();
        t.elapse(600).unwrap();
        t.finish().unwrap();
        assert_eq!(t.phase(), Phase::Finished);
        let s = t.take_completed_session().unwrap();
        assert_eq!(s.duration_secs, 600);
        assert_eq!(s.task, "写周报");
        assert_eq!(s.finished_naturally, false);
    }

    #[test]
    fn finish_after_natural_end_reports_full_duration() {
        let mut t = Timer::new();
        t.start("A".into(), 1800).unwrap();
        assert_eq!(t.elapse(1800).unwrap(), true);
        let s = t.take_completed_session().unwrap();
        assert_eq!(s.duration_secs, 1800);
        assert_eq!(s.finished_naturally, true);
    }

    #[test]
    fn finish_from_idle_invalid() {
        let mut t = Timer::new();
        assert_eq!(t.finish(), Err(TimerError::InvalidState));
    }

    #[test]
    fn reset_clears_state_and_task() {
        let mut t = Timer::new();
        t.start("A".into(), 1800).unwrap();
        t.reset();
        assert_eq!(t.phase(), Phase::Idle);
        assert_eq!(t.task(), None);
        assert_eq!(t.remaining_secs(), 0);
    }

    #[test]
    fn task_text_is_preserved_as_is() {
        let mut t = Timer::new();
        t.start("  写代码  ".into(), 1800).unwrap();
        assert_eq!(t.task(), Some("  写代码  "));
    }
}
