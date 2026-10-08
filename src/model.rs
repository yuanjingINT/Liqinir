use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const QUEUE_LIMIT: usize = 32;
pub const HISTORY_LIMIT: usize = 64;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Boot,
    NetworkConnected,
    NetworkDisconnected,
    PowerConnected,
    PowerDisconnected,
    Unlocked,
    DeviceConnected,
    DeviceDisconnected,
    Notification,
    Agent,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Event {
    #[serde(default)]
    pub id: u64,
    pub kind: Kind,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub priority: u8,
    #[serde(default = "default_ttl")]
    pub ttl_ms: u64,
    #[serde(default)]
    pub timestamp: u64,
}
fn default_ttl() -> u64 {
    5000
}

impl Event {
    pub fn new(kind: Kind, title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            id: 0,
            kind,
            title: title.into(),
            body: body.into(),
            source: "system".into(),
            priority: 1,
            ttl_ms: 5000,
            timestamp: 0,
        }
    }
    pub fn normalize(&mut self) {
        self.title = self.title.chars().take(160).collect();
        self.body = self.body.chars().take(4000).collect();
        self.source = self.source.chars().take(160).collect();
        self.priority = self.priority.min(3);
        self.ttl_ms = self.ttl_ms.clamp(1500, 30000);
        self.timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
    }
    fn same_content(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.source == other.source
            && self.title == other.title
            && self.body == other.body
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Status {
    pub network: Option<String>,
    pub battery: Option<u8>,
    pub charging: Option<bool>,
    pub active_window: Option<String>,
    pub locked: bool,
    pub reduced_motion: bool,
    pub providers: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Agent {
    pub busy: bool,
    pub prompt: String,
    pub reply: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    #[serde(rename = "type")]
    pub message_type: &'static str,
    pub current: Option<Event>,
    pub history: Vec<Event>,
    pub queued: usize,
    pub status: Status,
    pub agent: Agent,
}

pub struct State {
    pub status: Status,
    pub agent: Agent,
    pub current: Option<Event>,
    queue: VecDeque<Event>,
    history: VecDeque<Event>,
    pub deadline: Option<Instant>,
    paused_remaining: Option<Duration>,
    pub paused: bool,
    sequence: u64,
    recent: VecDeque<(Event, Instant)>,
}

impl State {
    pub fn new(reduced_motion: bool) -> Self {
        Self {
            status: Status {
                reduced_motion,
                ..Default::default()
            },
            agent: Agent::default(),
            current: None,
            queue: VecDeque::new(),
            history: VecDeque::new(),
            deadline: None,
            paused_remaining: None,
            paused: false,
            sequence: 0,
            recent: VecDeque::new(),
        }
    }
    pub fn snapshot(&self) -> Snapshot {
        let mut status = self.status.clone();
        if status.locked {
            status.active_window = None;
        }
        Snapshot {
            message_type: "state",
            current: if status.locked {
                None
            } else {
                self.current.clone()
            },
            history: if status.locked {
                vec![]
            } else {
                self.history.iter().rev().cloned().collect()
            },
            queued: self.queue.len(),
            status,
            agent: if self.status.locked {
                Agent::default()
            } else {
                self.agent.clone()
            },
        }
    }
    pub fn push(&mut self, mut event: Event, now: Instant) -> bool {
        event.normalize();
        while self
            .recent
            .front()
            .is_some_and(|(_, t)| now.duration_since(*t) >= Duration::from_secs(2))
        {
            self.recent.pop_front();
        }
        if self.recent.iter().any(|(old, _)| old.same_content(&event)) {
            return false;
        }
        self.sequence += 1;
        event.id = self.sequence;
        self.recent.push_back((event.clone(), now));
        if self.recent.len() > 64 {
            self.recent.pop_front();
        }
        self.history.push_back(event.clone());
        if self.history.len() > HISTORY_LIMIT {
            self.history.pop_front();
        }
        if self.status.locked {
            return true;
        }
        if self.current.is_none() {
            self.show(event, now);
        } else if self
            .current
            .as_ref()
            .is_some_and(|c| event.priority > c.priority)
        {
            // Preempted events remain in history; do not replay obsolete status.
            self.show(event, now);
        } else {
            if event.kind != Kind::Notification && event.kind != Kind::Agent {
                self.queue.retain(|e| e.kind != event.kind);
            }
            if self.queue.len() >= QUEUE_LIMIT {
                let index = self.queue.iter().position(|e| e.priority < 3).unwrap_or(0);
                self.queue.remove(index);
            }
            let index = self
                .queue
                .iter()
                .position(|e| e.priority < event.priority)
                .unwrap_or(self.queue.len());
            self.queue.insert(index, event);
        }
        true
    }
    fn show(&mut self, event: Event, now: Instant) {
        let duration = Duration::from_millis(event.ttl_ms);
        self.deadline = if self.paused {
            None
        } else {
            Some(now + duration)
        };
        self.paused_remaining = self.paused.then_some(duration);
        self.current = Some(event);
    }
    pub fn advance(&mut self, now: Instant) {
        self.current = None;
        self.deadline = None;
        self.paused_remaining = None;
        if !self.status.locked
            && let Some(event) = self.queue.pop_front()
        {
            self.show(event, now);
        }
    }
    pub fn pause(&mut self, paused: bool, now: Instant) {
        if paused == self.paused {
            return;
        }
        self.paused = paused;
        if paused {
            self.paused_remaining = self.deadline.map(|d| d.saturating_duration_since(now));
            self.deadline = None;
        } else if self.current.is_some() {
            self.deadline = Some(
                now + self
                    .paused_remaining
                    .take()
                    .unwrap_or(Duration::from_secs(5)),
            );
        }
    }
    pub fn lock(&mut self, locked: bool, now: Instant) {
        if self.status.locked == locked {
            return;
        }
        self.status.locked = locked;
        self.queue.clear();
        self.current = None;
        self.deadline = None;
        self.paused = false;
        self.paused_remaining = None;
        if !locked {
            self.push(Event::new(Kind::Unlocked, "欢迎回来", "桌面已解锁"), now);
        }
    }
    pub fn clear_history(&mut self) {
        self.history.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn critical_preempts_and_duplicate_is_suppressed() {
        let mut s = State::new(false);
        let t = Instant::now();
        let event = Event::new(Kind::Notification, "hello", "world");
        assert!(s.push(event.clone(), t));
        assert!(!s.push(event, t));
        let mut critical = Event::new(Kind::PowerDisconnected, "low battery", "");
        critical.priority = 3;
        s.push(critical, t);
        assert_eq!(s.current.as_ref().unwrap().title, "low battery");
    }
    #[test]
    fn flood_is_bounded_and_fifo_at_equal_priority() {
        let mut s = State::new(false);
        let t = Instant::now();
        for i in 0..200 {
            s.push(Event::new(Kind::Notification, i.to_string(), ""), t);
        }
        assert_eq!(s.snapshot().queued, QUEUE_LIMIT);
        assert_eq!(s.snapshot().history.len(), HISTORY_LIMIT);
        s.advance(t);
        assert_eq!(s.current.unwrap().title, "168");
    }
    #[test]
    fn lock_redacts_content_and_does_not_replay_private_popups() {
        let mut s = State::new(false);
        let t = Instant::now();
        s.agent.reply = "secret".into();
        s.status.active_window = Some("secret window".into());
        s.push(Event::new(Kind::Notification, "private", "secret"), t);
        s.lock(true, t);
        s.push(Event::new(Kind::Notification, "private2", "secret2"), t);
        let snapshot = s.snapshot();
        assert!(snapshot.current.is_none());
        assert!(snapshot.history.is_empty());
        assert!(snapshot.agent.reply.is_empty());
        assert!(snapshot.status.active_window.is_none());
        s.lock(false, t);
        assert_eq!(s.snapshot().queued, 0);
        assert_eq!(s.current.unwrap().kind, Kind::Unlocked);
    }
    #[test]
    fn pause_preserves_remaining_time() {
        let mut s = State::new(false);
        let t = Instant::now();
        s.push(Event::new(Kind::Boot, "hello", ""), t);
        s.pause(true, t + Duration::from_secs(2));
        assert!(s.deadline.is_none());
        s.pause(false, t + Duration::from_secs(20));
        assert_eq!(s.deadline.unwrap(), t + Duration::from_secs(23));
    }
}
