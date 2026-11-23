#![no_std]
use aidoku::{Source, prelude::*};
use madara::{Impl, Madara, Params};

struct NinjaComics;

impl Impl for NinjaComics {
	fn new() -> Self {
		Self
	}

	fn params(&self) -> Params {
		Params {
			base_url: "https://ninjacomics.xyz".into(),
			..Default::default()
		}
	}
}

register_source!(Madara<NinjaComics>, ListingProvider, DeepLinkHandler);
