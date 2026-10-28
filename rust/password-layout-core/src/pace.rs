//! How long the service loop waits after a look, copied from `nextPace` in
//! src/watch.cpp.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pace {
    SecondLook,
    Poll,
    Sleep,
}

pub fn next_pace(second_look_owed: bool, pending: bool, busy: bool, can_wait: bool) -> Pace {
    if second_look_owed && !pending {
        return Pace::SecondLook;
    }
    if pending || busy || !can_wait {
        return Pace::Poll;
    }
    Pace::Sleep
}
