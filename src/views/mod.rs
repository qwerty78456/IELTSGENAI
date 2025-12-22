//! The views module contains the components for all Layouts and Routes for our app.
//!
//! The [`Home`] component will be rendered as the main IELTS generation page.
//!
//! The [`Navbar`] component will be rendered on all pages of our app.

mod home;
pub use home::Home;

mod navbar;
pub use navbar::Navbar;
