#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceStatus {
    Healthy,
    Lost,
    Outdated,
    Timeout,
    OutOfMemory,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleEvent {
    Started,
    Suspended,
    Resumed,
    FocusLost,
    FocusGained,
    Resized { width: u32, height: u32 },
    SurfaceChanged(SurfaceStatus),
    SurfaceRecovered,
    Terminating,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleState {
    started: bool,
    focused: bool,
    suspended: bool,
    surface: SurfaceStatus,
    width: u32,
    height: u32,
}

impl Default for LifecycleState {
    fn default() -> Self {
        Self {
            started: false,
            // Treat the shell as focused until the platform says otherwise so
            // the first real focus-loss event is observable.
            focused: true,
            suspended: false,
            surface: SurfaceStatus::Healthy,
            width: 0,
            height: 0,
        }
    }
}

impl LifecycleState {
    pub fn start(&mut self) -> Option<LifecycleEvent> {
        if self.started {
            None
        } else {
            self.started = true;
            Some(LifecycleEvent::Started)
        }
    }

    pub fn set_focused(&mut self, focused: bool) -> Option<LifecycleEvent> {
        if self.focused == focused {
            None
        } else {
            self.focused = focused;
            Some(if focused {
                LifecycleEvent::FocusGained
            } else {
                LifecycleEvent::FocusLost
            })
        }
    }

    pub fn set_suspended(&mut self, suspended: bool) -> Option<LifecycleEvent> {
        if self.suspended == suspended {
            None
        } else {
            self.suspended = suspended;
            Some(if suspended {
                LifecycleEvent::Suspended
            } else {
                LifecycleEvent::Resumed
            })
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Option<LifecycleEvent> {
        if self.width == width && self.height == height {
            None
        } else {
            self.width = width;
            self.height = height;
            Some(LifecycleEvent::Resized { width, height })
        }
    }

    pub fn surface_status(&mut self, status: SurfaceStatus) -> Option<LifecycleEvent> {
        let previous = self.surface;
        self.surface = status;
        if previous == status {
            return None;
        }
        match status {
            SurfaceStatus::Healthy if previous != SurfaceStatus::Healthy => {
                Some(LifecycleEvent::SurfaceRecovered)
            }
            SurfaceStatus::Healthy => None,
            status => Some(LifecycleEvent::SurfaceChanged(status)),
        }
    }

    pub fn started(&self) -> bool {
        self.started
    }

    pub fn focused(&self) -> bool {
        self.focused
    }

    pub fn suspended(&self) -> bool {
        self.suspended
    }

    pub fn surface(&self) -> SurfaceStatus {
        self.surface
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_transitions_are_coalesced_and_stateful() {
        let mut state = LifecycleState::default();
        assert_eq!(state.start(), Some(LifecycleEvent::Started));
        assert_eq!(state.start(), None);
        assert_eq!(state.set_focused(false), Some(LifecycleEvent::FocusLost));
        assert_eq!(state.set_focused(false), None);
        assert_eq!(state.set_suspended(true), Some(LifecycleEvent::Suspended));
        assert!(state.suspended());
        assert_eq!(state.set_suspended(false), Some(LifecycleEvent::Resumed));
        assert!(!state.suspended());
    }

    #[test]
    fn surface_recovery_is_distinct_from_surface_failure() {
        let mut state = LifecycleState::default();
        assert_eq!(
            state.surface_status(SurfaceStatus::Lost),
            Some(LifecycleEvent::SurfaceChanged(SurfaceStatus::Lost))
        );
        assert_eq!(
            state.surface_status(SurfaceStatus::Healthy),
            Some(LifecycleEvent::SurfaceRecovered)
        );
    }
}
