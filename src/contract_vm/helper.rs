use std::fmt::{self, Display};
use std::ops::{Deref, DerefMut};

/// A macro that creates lazy variables, root export
///
/// # Usage
///
/// ```ignore
/// lazy_mut! {
///     // Local variables
///     let mut NAME: TY = EXPR;
///
///     // Static variables
///     [pub [(VIS)]] static mut NAME: TY = EXPR;
/// }
/// ```
#[macro_export]
macro_rules! lazy_mut {
    (/* empty */) => {};
    ($(#[$attr:meta])* $D:ident mut $N:ident: $T:ty = $e:expr; $($t:tt)*) => {
        $(#[$attr])*
        $D mut $N: $crate::contract_vm::helper::LazyMut<$T> = {
            $crate::contract_vm::helper::LazyMut::Init(||$e)
        };
        lazy_mut!($($t)*);
    };
}

/// A mutable lazy value with either an initializer or a value
///
/// See the module-level documentation for more information on usage.
#[derive(Clone, Debug)]
pub enum LazyMut<T> {
    /// An initializer that will be run to obtain the first value
    Init(fn() -> T),
    /// The value from the initializer
    Value(T),
}

impl<T> LazyMut<T> {
    /// Returns the wrapped value, initializing if needed
    pub fn unwrap(self) -> T {
        use LazyMut::*;
        match self {
            Init(init) => init(),
            Value(val) => val,
        }
    }

    /// Initializes the wrapped value if it is uninitialized
    pub fn init(&mut self) -> &mut LazyMut<T> {
        use LazyMut::*;
        let new = match self {
            &mut Init(init) => Value(init()),
            other => return other,
        };
        *self = new;
        self
    }

    /// Initializes the wrapped value, panicking if it was already initialized
    pub fn init_once(&mut self) -> &mut LazyMut<T> {
        use LazyMut::*;
        let new = match self {
            &mut Init(init) => Value(init()),
            _ => panic!("call to `init_once` on already initialized value"),
        };
        *self = new;
        self
    }

    /// Tries to get a reference to the value, returns `None` if the value is uninitialized
    ///
    /// Uses associated function syntax (`LazyMut::get(&VAL)`)
    pub fn get(this: &LazyMut<T>) -> Option<&T> {
        use LazyMut::*;
        match this {
            &Init(_) => None,
            &Value(ref val) => Some(val),
        }
    }

    /// Tries to get a mutable reference the value, returns `None` if the value is uninitialized
    ///
    /// Uses associated function syntax (`LazyMut::get_mut(&mut VAL)`)
    pub fn get_mut(this: &mut LazyMut<T>) -> Option<&mut T> {
        use LazyMut::*;
        match this {
            &mut Init(_) => None,
            &mut Value(ref mut val) => Some(val),
        }
    }

    /// Returns `true` if the wrapped value has been initialized
    pub fn is_initialized(&self) -> bool {
        use LazyMut::*;
        match self {
            &Init(_) => false,
            &Value(_) => true,
        }
    }
}

impl<T> Deref for LazyMut<T> {
    type Target = T;
    fn deref(&self) -> &T {
        use LazyMut::*;
        match self {
            &Init(_) => panic!("cannot dereference uninitialized value"),
            &Value(ref val) => val,
        }
    }
}

impl<T> DerefMut for LazyMut<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.init();
        use LazyMut::*;
        match self {
            &mut Init(_) => unreachable!(),
            &mut Value(ref mut val) => val,
        }
    }
}

impl<T> Display for LazyMut<T>
where
    T: Display,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use LazyMut::*;
        match self {
            &Init(_) => write!(f, "{{uninitialized}}"),
            &Value(ref val) => val.fmt(f),
        }
    }
}
