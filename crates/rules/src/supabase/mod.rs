//! The two rules that read a Supabase project the way an attacker reads it:
//! which key is in the bundle, and which table anybody holding a key can select
//! from.
//!
//! Supabase puts the whole database one HTTP call from the browser and defends
//! it with two things, the key the client carries and the row-level-security
//! policies on each table. Both rules ask about one of those, and both are
//! A01:2021 (broken access control), CWE-284, High severity and High
//! confidence: the shapes they match are unambiguous, and the consequence of
//! either is the same, every row of the table readable by anyone who opens the
//! app.
//!
//! [`service_role`] is a File rule, because whether a key is in the bundle is a
//! question about one file. [`rls`] is a Graph rule, because whether a table is
//! locked down is a question about every migration at once: a table created in
//! January and secured in March is secure, and no single file says so.

pub mod rls;
pub mod service_role;
