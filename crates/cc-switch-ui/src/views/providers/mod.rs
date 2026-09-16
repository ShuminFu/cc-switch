//! Provider list for the active app. Port of
//! `src/components/providers/ProviderList.tsx`, `ProviderCard.tsx`,
//! `ProviderActions.tsx` and the header toggles on the providers view.

pub mod actions;
mod card;
pub mod empty_state;
pub mod form;
pub mod header_toggles;
mod list;
pub mod profile_switcher;

pub use list::ProvidersView;
