#![no_std]
use aidoku::{Source, prelude::*};
use madara::{Impl, Madara, Params};

struct LerMangas;

impl Impl for LerMangas {
	fn new() -> Self {
		Self
	}

	fn params(&self) -> Params {
		Params {
			base_url: "https://lermangas.me".into(),
			..Default::default()
		}
	}
}

register_source!(Madara<LerMangas>, ListingProvider, DeepLinkHandler);
