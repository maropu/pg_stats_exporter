pub mod logging;
pub mod metrics;
pub mod postgres_connection;
pub mod routes;
pub mod tcp_listener;
pub mod tracing_utils;

/// This is a shortcut to embed git sha into binaries and avoid copying the same build script to all packages
///
/// we have several cases:
/// * building locally from git repo
/// * building in CI from git repo
/// * building in docker (either in CI or locally)
///
/// One thing to note is that .git is not available in docker (and it is bad to include it there).
/// When building locally, the `git_version` is used to query .git. When building on CI and docker,
/// we don't build the actual PR branch commits, but always a "phantom" would be merge commit to
/// the target branch -- the actual PR commit from which we build from is supplied as GIT_VERSION
/// environment variable.
///
/// We ended up with this compromise between phantom would be merge commits vs. pull request branch
/// heads due to old logs becoming more reliable (github could gc the phantom merge commit
/// anytime) in #4641.
///
/// To avoid running buildscript every recompilation, we use rerun-if-env-changed option.
/// So the build script will be run only when GIT_VERSION envvar has changed.
///
/// Why not to use buildscript to get git commit sha directly without procmacro from different crate?
/// Caching and workspaces complicates that. In case `utils` is not
/// recompiled due to caching then version may become outdated.
/// git_version crate handles that case by introducing a dependency on .git internals via include_bytes! macro,
/// so if we changed the index state git_version will pick that up and rerun the macro.
///
/// Note that with git_version prefix is `git:` and in case of git version from env its `git-env:`.
///
/// #############################################################################################
/// TODO this macro is not the way the library is intended to be used, see <https://github.com/neondatabase/neon/issues/1565> for details.
/// We use `cachepot` to reduce our current CI build times: <https://github.com/neondatabase/cloud/pull/1033#issuecomment-1100935036>
/// Yet, it seems to ignore the GIT_VERSION env variable, passed to Docker build, even with build.rs that contains
/// `println!("cargo:rerun-if-env-changed=GIT_VERSION");` code for cachepot cache invalidation.
/// The problem needs further investigation and regular `const` declaration instead of a macro.
#[macro_export]
macro_rules! project_git_version {
    ($const_identifier:ident) => {
        // this should try GIT_VERSION first only then git_version::git_version!
        const $const_identifier: &::core::primitive::str = {
            const __COMMIT_FROM_GIT: &::core::primitive::str = git_version::git_version! {
                prefix = "",
                fallback = "unknown",
                args = ["--abbrev=40", "--always", "--dirty=-modified"] // always use full sha
            };

            const __ARG: &[&::core::primitive::str; 2] = &match ::core::option_env!("GIT_VERSION") {
                ::core::option::Option::Some(x) => ["git-env:", x],
                ::core::option::Option::None => ["git:", __COMMIT_FROM_GIT],
            };

            $crate::__const_format::concatcp!(__ARG[0], __ARG[1])
        };
    };
}

/// Re-export for `project_git_version` macro
#[doc(hidden)]
pub use const_format as __const_format;
