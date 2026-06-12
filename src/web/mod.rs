pub mod admin;
pub mod api;
pub mod bulk;
pub mod middleware;
pub mod pages;
pub mod password_gate;
pub mod qr;
pub mod redirect;
pub mod routes;
pub mod system;

pub use routes::create_router;
