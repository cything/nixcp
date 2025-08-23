use anyhow::{Context, Result, anyhow, bail};
use argon2::{
    Argon2,
    password_hash::{PasswordHasher, SaltString, rand_core::OsRng},
};
use rand::prelude::*;
use rpassword::prompt_password;

use crate::HashArgs;

const PASSWORD_LEN: usize = 32;

pub fn run(cli: &HashArgs) -> Result<()> {
    let mut password = String::new();

    if !cli.random {
        let password = prompt_password("Password: ").context("prompt for password")?;
        if !cli.no_confirm {
            let confirm_password =
                prompt_password("Confirm: ").context("prompt for confirm password")?;
            if password != confirm_password {
                bail!("Passwords do not match.");
            }
        }
    } else {
        password = generate_password();
    }

    let pw_hash = hash_password(&password)?;
    if cli.random {
        println!("Password: {password}");
    }
    println!("Hash: {pw_hash}");

    Ok(())
}

fn generate_password() -> String {
    let mut rng = rand::rng();
    let charset: Vec<char> = "ABCDEFGHIJKLMNOPQRSTUVWXYZ\
                              abcdefghijklmnopqrstuvwxyz\
                              0123456789"
        .chars()
        .collect();

    (0..PASSWORD_LEN)
        .map(|_| {
            charset
                .choose(&mut rng)
                .expect("constant CHARSET is not empty")
        })
        .collect()
}

fn hash_password(password: &str) -> Result<String> {
    let argon2 = Argon2::default();
    let salt = SaltString::generate(&mut OsRng);
    argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow!(e.to_string()))
        .map(|x| x.to_string())
}
