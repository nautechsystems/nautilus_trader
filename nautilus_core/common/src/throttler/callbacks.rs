use std::{cell::RefCell, rc::Rc};

use super::inner::InnerThrottler;
use crate::timer::{TimeEvent, TimeEventCallback};

pub struct ThrottlerResume<T, F> {
    inner: Rc<RefCell<InnerThrottler<T, F>>>,
}

impl<T, F> ThrottlerResume<T, F> {
    pub const fn new(inner: Rc<RefCell<InnerThrottler<T, F>>>) -> Self {
        Self { inner }
    }
}

impl<T: 'static, F: Fn(T) + 'static> From<ThrottlerResume<T, F>> for TimeEventCallback {
    fn from(value: ThrottlerResume<T, F>) -> Self {
        Self::Rust(Rc::new(move |_event: TimeEvent| {
            value.inner.borrow_mut().is_limiting = false;
        }))
    }
}

#[derive(Clone)]
pub struct ThrottlerProcess<T, F> {
    inner: Rc<RefCell<InnerThrottler<T, F>>>,
}

impl<T, F> ThrottlerProcess<T, F> {
    pub const fn new(inner: Rc<RefCell<InnerThrottler<T, F>>>) -> Self {
        Self { inner }
    }
}

impl<T: 'static, F: Fn(T) + 'static> From<ThrottlerProcess<T, F>> for TimeEventCallback {
    fn from(value: ThrottlerProcess<T, F>) -> Self {
        Self::Rust(Rc::new(move |_event: TimeEvent| {
            let process_clone = ThrottlerProcess {
                inner: value.inner.clone(),
            };
            let mut core = value.inner.borrow_mut();
            while let Some(msg) = core.buffer.pop_back() {
                core.send_msg(msg);

                if core.delta_next() > 0 {
                    core.set_timer(Some(process_clone.into()));
                    return;
                }
            }

            core.is_limiting = false;
        }))
    }
}
