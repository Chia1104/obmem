//! A form's problems: the rules its values break, shown at the fields they are about. Native
//! Shell has no form layer, so this is the app's, and it is small: the rules themselves are
//! obmem-core's (`ProjectSettings::problems` and the like), so what a form lets through is
//! exactly what a save accepts.

use nsh_core::prelude::*;
use obmem_core::settings::Problem;

/// The problems of a form's values, by field. None shows before the form is first sent, so an
/// untouched form is not red. After that they follow what is typed: a problem goes as soon as its
/// field is put right.
pub(crate) struct Problems<F: 'static> {
    tried: Signal<bool>,
    found: Memo<Vec<Problem<F>>>,
}

impl<F> Clone for Problems<F> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<F> Copy for Problems<F> {}

impl<F: Copy + PartialEq + 'static> Problems<F> {
    /// `check` reads the form's signals and gives every rule their values break.
    pub(crate) fn of(check: impl Fn() -> Vec<Problem<F>> + 'static) -> Self {
        Problems {
            tried: signal(false),
            found: computed(check),
        }
    }

    /// The problem at `field`, for `widgets::checked_field`.
    pub(crate) fn at(self, field: F) -> impl Fn() -> Option<String> + Copy + 'static {
        move || {
            if !self.tried.get() {
                return None;
            }
            self.found.with(|all| {
                all.iter()
                    .find(|p| p.field == field)
                    .map(|p| p.message.clone())
            })
        }
    }

    /// Whether `field` has a problem, for `widgets::Marked`.
    pub(crate) fn marks(self, field: F) -> impl Fn() -> bool + Copy + 'static {
        let at = self.at(field);
        move || at().is_some()
    }

    /// The form is being sent: from now on its problems show. Whether it has none.
    pub(crate) fn none(self) -> bool {
        self.tried.set(true);
        untracked(|| self.found.with(Vec::is_empty))
    }
}
