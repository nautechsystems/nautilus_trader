use std::{cell::RefCell, rc::Rc};

use crate::timer::{RustTimeEventCallback, TimeEvent, TimeEventCallback};

use super::inner::InnerThrottler;

pub struct ThrottlerResume<T, F> {
    inner: Rc<RefCell<InnerThrottler<T, F>>>,
}

impl<T, F> ThrottlerResume<T, F> {
    pub fn new(inner: Rc<RefCell<InnerThrottler<T, F>>>) -> Self {
        Self { inner }
    }
}

impl<T: 'static, F: Fn(T) + 'static> From<ThrottlerResume<T, F>> for TimeEventCallback {
    fn from(value: ThrottlerResume<T, F>) -> Self {
        TimeEventCallback::Rust(Rc::new(value))
    }
}

impl<T, F> RustTimeEventCallback for ThrottlerResume<T, F> {
    fn call(&self, _event: TimeEvent) {
        self.inner.borrow_mut().is_limiting = false;
    }
}

#[derive(Clone)]
pub struct ThrottlerProcess<T, F> {
    inner: Rc<RefCell<InnerThrottler<T, F>>>,
}

impl<T, F> RustTimeEventCallback for ThrottlerProcess<T, F>
where
    F: Fn(T) + 'static,
    T: 'static,
{
    fn call(&self, _event: TimeEvent) {
        let process_clone = ThrottlerProcess {
            inner: self.inner.clone(),
        };
        let mut core = self.inner.borrow_mut();
        while let Some(msg) = core.buffer.pop_back() {
            core.send_msg(msg);

            if core.delta_next() > 0 {
                core.set_timer(Some(process_clone.into()));
                return;
            }
        }

        core.is_limiting = false;
    }
}

impl<T: 'static, F: Fn(T) + 'static> From<ThrottlerProcess<T, F>> for TimeEventCallback {
    fn from(value: ThrottlerProcess<T, F>) -> Self {
        TimeEventCallback::Rust(Rc::new(value))
    }
}

impl<T, F> ThrottlerProcess<T, F> {
    pub fn new(inner: Rc<RefCell<InnerThrottler<T, F>>>) -> Self {
        Self { inner }
    }
}
