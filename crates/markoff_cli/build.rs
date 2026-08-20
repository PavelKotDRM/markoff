use vergen_gitcl::{Build, Cargo, Emitter, Gitcl, Rustc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let build = Build::builder().build_timestamp(true).build();
    let cargo = Cargo::builder().target_triple(true).build();
    let git = Gitcl::builder().branch(true).sha(true).build();
    let rustc = Rustc::builder().semver(true).build();

    Emitter::default()
        .default_on_error()
        .add_instructions(&build)?
        .add_instructions(&cargo)?
        .add_instructions(&git)?
        .add_instructions(&rustc)?
        .emit()?;
    Ok(())
}
