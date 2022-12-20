//! Posix thread implementation

use core::{
    any::Any,
    sync::atomic::{AtomicI32, Ordering},
};

use jinux_frame::task::Task;

use crate::prelude::*;

use self::status::ThreadStatus;

pub mod exception;
pub mod kernel_thread;
pub mod status;
pub mod task;
pub mod thread_table;

pub type Tid = i32;

static TID_ALLOCATOR: AtomicI32 = AtomicI32::new(0);

/// A thread is a wrapper on top of task.
pub struct Thread {
    // immutable part
    /// Thread id
    tid: Tid,
    /// Low-level info
    task: Arc<Task>,
    /// Data: Posix thread info/Kernel thread Info
    data: Box<dyn Send + Sync + Any>,

    // mutable part
    status: Mutex<ThreadStatus>,
}

impl Thread {
    /// Never call these function directly
    pub fn new(
        tid: Tid,
        task: Arc<Task>,
        data: impl Send + Sync + Any,
        status: ThreadStatus,
    ) -> Self {
        Thread {
            tid,
            task,
            data: Box::new(data),
            status: Mutex::new(status),
        }
    }

    pub fn current() -> Arc<Self> {
        let task = Task::current();
        let thread = task
            .data()
            .downcast_ref::<Weak<Thread>>()
            .expect("[Internal Error] task data should points to weak<thread>");
        thread
            .upgrade()
            .expect("[Internal Error] current thread cannot be None")
    }

    /// Add inner task to the run queue of scheduler. Note this does not means the thread will run at once.
    pub fn run(&self) {
        self.status.lock().set_running();
        self.task.run();
    }

    pub fn exit(&self) {
        let mut status = self.status.lock();
        if !status.is_exited() {
            status.set_exited();
        }
    }

    pub fn status(&self) -> &Mutex<ThreadStatus> {
        &self.status
    }

    pub fn yield_now() {
        Task::yield_now()
    }

    pub fn tid(&self) -> Tid {
        self.tid
    }

    pub fn data(&self) -> &Box<dyn Send + Sync + Any> {
        &self.data
    }
}

/// allocate a new pid for new process
pub fn allocate_tid() -> Tid {
    TID_ALLOCATOR.fetch_add(1, Ordering::SeqCst)
}
