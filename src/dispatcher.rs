use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    thread::ThreadId,
    time::{Duration, Instant},
};

use bevy_tasks::AsyncComputeTaskPool;
use gpui::{PlatformDispatcher, Priority, RunnableVariant, ThreadTaskTimings};

type WakeEventLoop = Arc<dyn Fn() + Send + Sync>;

struct DelayedRunnable {
    deadline: Instant,
    runnable: RunnableVariant,
}

pub(crate) struct BevyDispatcher {
    main_thread: ThreadId,
    main_queue: Arc<Mutex<VecDeque<RunnableVariant>>>,
    delayed: Arc<Mutex<Vec<DelayedRunnable>>>,
    wake_event_loop: Option<WakeEventLoop>,
}

impl BevyDispatcher {
    #[cfg(feature = "render")]
    pub(crate) fn new(wake_event_loop: Option<WakeEventLoop>) -> Arc<Self> {
        Arc::new(Self {
            main_thread: std::thread::current().id(),
            main_queue: Arc::new(Mutex::new(VecDeque::new())),
            delayed: Arc::new(Mutex::new(Vec::new())),
            wake_event_loop,
        })
    }

    pub(crate) fn drain_main_queue(&self, limit: usize) -> usize {
        assert_eq!(
            std::thread::current().id(),
            self.main_thread,
            "GPUI main-thread tasks must be drained on Bevy's main thread"
        );
        let now = Instant::now();
        let mut delayed = self.delayed.lock().unwrap();
        let mut index = 0;
        while index < delayed.len() {
            if delayed[index].deadline <= now {
                let delayed = delayed.swap_remove(index);
                self.main_queue.lock().unwrap().push_back(delayed.runnable);
            } else {
                index += 1;
            }
        }
        drop(delayed);

        let mut drained = 0;
        while drained < limit {
            let runnable = self.main_queue.lock().unwrap().pop_front();
            let Some(runnable) = runnable else {
                break;
            };
            runnable.run();
            drained += 1;
        }
        drained
    }

    fn wake(&self) {
        if let Some(wake) = &self.wake_event_loop {
            wake();
        }
    }

    fn spawn_background(task: impl FnOnce() + Send + 'static) {
        if let Some(pool) = AsyncComputeTaskPool::try_get() {
            pool.spawn(async move { task() }).detach();
        } else {
            std::thread::spawn(task);
        }
    }
}

impl PlatformDispatcher for BevyDispatcher {
    fn get_all_timings(&self) -> Vec<ThreadTaskTimings> {
        Vec::new()
    }

    fn get_current_thread_timings(&self) -> ThreadTaskTimings {
        ThreadTaskTimings {
            thread_name: std::thread::current().name().map(str::to_owned),
            thread_id: std::thread::current().id(),
            timings: Vec::new(),
            total_pushed: 0,
        }
    }

    fn is_main_thread(&self) -> bool {
        std::thread::current().id() == self.main_thread
    }

    fn dispatch(&self, runnable: RunnableVariant, _priority: Priority) {
        Self::spawn_background(move || {
            runnable.run();
        });
    }

    fn dispatch_on_main_thread(&self, runnable: RunnableVariant, _priority: Priority) {
        self.main_queue.lock().unwrap().push_back(runnable);
        self.wake();
    }

    fn dispatch_after(&self, duration: Duration, runnable: RunnableVariant) {
        self.delayed.lock().unwrap().push(DelayedRunnable {
            deadline: Instant::now() + duration,
            runnable,
        });
        let wake = self.wake_event_loop.clone();
        if let Some(pool) = AsyncComputeTaskPool::try_get() {
            pool.spawn(async move {
                futures_timer::Delay::new(duration).await;
                if let Some(wake) = wake {
                    wake();
                }
            })
            .detach();
        } else {
            std::thread::spawn(move || {
                std::thread::sleep(duration);
                if let Some(wake) = wake {
                    wake();
                }
            });
        }
    }

    fn spawn_realtime(&self, task: Box<dyn FnOnce() + Send>) {
        std::thread::spawn(task);
    }
}
