use std::{
    fs::{self, File},
    io::{self, prelude::*},
    path::{Path, PathBuf},
};

use clap::Parser;
use groth16_solana::groth16::Groth16Verifyingkey;
use quote::quote;

use crate::utils::rustfmt;

#[derive(Debug, Parser)]
pub struct Options {
    #[clap(long)]
    output_path: PathBuf,
    #[clap(long)]
    input_path: PathBuf,
}

pub fn create_vkeyrs_from_gnark_key(opts: Options) -> anyhow::Result<()> {
    let gnark_vk_bytes = read_array_from_file(opts.input_path)?;
    let vk = read_gnark_vk_bytes(&gnark_vk_bytes);

    let nr_pubinputs = vk.nr_pubinputs;
    let vk_alpha_g1 = vk.vk_alpha_g1.iter().map(|b| quote! {#b});
    let vk_beta_g2 = vk.vk_beta_g2.iter().map(|b| quote! {#b});
    let vk_gamme_g2 = vk.vk_gamme_g2.iter().map(|b| quote! {#b});
    let vk_delta_g2 = vk.vk_delta_g2.iter().map(|b| quote! {#b});
    let vk_ic_slices = vk.vk_ic.iter().map(|slice| {
        let bytes = slice.iter().map(|b| quote! {#b});
        quote! {[#(#bytes),*]}
    });

    // Now use these parts with the quote! macro
    let code = quote! {
        use groth16_solana::groth16::Groth16Verifyingkey;

        pub const VERIFYINGKEY: Groth16Verifyingkey = Groth16Verifyingkey {
            nr_pubinputs: #nr_pubinputs,
            vk_alpha_g1: [#(#vk_alpha_g1),*],
            vk_beta_g2: [#(#vk_beta_g2),*],
            vk_gamme_g2: [#(#vk_gamme_g2),*],
            vk_delta_g2: [#(#vk_delta_g2),*],
            vk_ic: &[#(#vk_ic_slices),*],
        };
    };

    let mut file = File::create(&opts.output_path)?;
    file.write_all(b"// This file is generated by xtask. Do not edit it manually.\n\n")?;
    file.write_all(&rustfmt(code.to_string())?)?;
    Ok(())
}

fn read_array_from_file<P: AsRef<Path>>(file_path: P) -> io::Result<Vec<u8>> {
    // Read the entire file to a String
    let contents = fs::read_to_string(file_path)?;

    // Parse the string as a Vec<u8>
    let array = contents
        .trim_matches(|p| p == '[' || p == ']')
        .split(' ')
        .map(str::trim)
        .filter_map(|s| s.parse::<u8>().ok())
        .collect::<Vec<u8>>();

    Ok(array)
}
pub fn read_gnark_vk_bytes<'a>(gnark_vk_bytes: &[u8]) -> Groth16Verifyingkey<'a> {
    // layout of vk:
    // [α]1,[β]1,[β]2,[γ]2,[δ]1,[δ]2, K/IC[num_pubinputs + 1]
    let alpha_g1_offset_start: usize = 0;
    let alpha_g1_offset_end: usize = 64;

    let beta_g2_offset_start: usize = 128;
    let beta_g2_offset_end: usize = 256;

    let gamma_g2_offset_start: usize = 256;
    let gamma_g2_offset_end: usize = 384;

    let delta_g2_offset_start: usize = 384 + 64;
    let delta_g2_offset_end: usize = 512 + 64;

    // K offsets (each element is 64 bytes)
    let nr_k_offset_start: usize = 512 + 64;
    let nr_k_offset_end: usize = 512 + 64 + 4;
    let k_offset_start: usize = 512 + 64 + 4;
    let nr_pubinputs: usize = u32::from_be_bytes(
        gnark_vk_bytes[nr_k_offset_start..nr_k_offset_end]
            .try_into()
            .unwrap(),
    )
    .try_into()
    .unwrap();
    let gamma_g2_be = gnark_vk_bytes[gamma_g2_offset_start..gamma_g2_offset_end]
        .try_into()
        .unwrap();
    let delta_g2_be = gnark_vk_bytes[delta_g2_offset_start..delta_g2_offset_end]
        .try_into()
        .unwrap();
    let mut vk_ic = Vec::<[u8; 64]>::new();
    for i in 0..nr_pubinputs {
        vk_ic.push(
            gnark_vk_bytes[k_offset_start + i * 64..k_offset_start + (i + 1) * 64]
                .try_into()
                .unwrap(),
        );
    }
    let vk_ic = Box::new(vk_ic);
    let vk_ic_slice: &'a [[u8; 64]] = Box::leak(vk_ic);

    Groth16Verifyingkey {
        nr_pubinputs: nr_pubinputs - 1,
        vk_alpha_g1: gnark_vk_bytes[alpha_g1_offset_start..alpha_g1_offset_end]
            .try_into()
            .unwrap(),
        vk_beta_g2: gnark_vk_bytes[beta_g2_offset_start..beta_g2_offset_end]
            .try_into()
            .unwrap(),
        vk_gamme_g2: gamma_g2_be,
        vk_delta_g2: delta_g2_be,
        vk_ic: vk_ic_slice,
    }
}
