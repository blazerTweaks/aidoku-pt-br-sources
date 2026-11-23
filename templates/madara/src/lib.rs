#![no_std]
//! Template para sites WordPress com o tema Madara.
//!
//! Uso numa fonte:
//! ```ignore
//! use madara::{Impl, Madara, Params};
//!
//! struct MinhaFonte;
//!
//! impl Impl for MinhaFonte {
//! 	fn new() -> Self {
//! 		Self
//! 	}
//!
//! 	fn params(&self) -> Params {
//! 		Params {
//! 			base_url: "https://exemplo.com".into(),
//! 			..Default::default()
//! 		}
//! 	}
//! }
//!
//! register_source!(Madara<MinhaFonte>, ListingProvider, DeepLinkHandler);
//! ```
//!
//! Qualquer método de [`Impl`] pode ser sobrescrito para tratar diferenças de um site.
use aidoku::{
	Chapter, DeepLinkHandler, DeepLinkResult, FilterValue, Listing, ListingProvider, Manga,
	MangaPageResult, Page, Result, Source,
	alloc::{String, Vec, borrow::Cow},
};

pub mod helpers;
mod imp;

pub use imp::Impl;

pub struct Params {
	/// URL base do site, sem barra final (ex.: `https://exemplo.com`).
	pub base_url: Cow<'static, str>,
	/// Caminho das obras no site; o padrão do Madara é `manga`.
	pub source_path: Cow<'static, str>,
	/// Número mínimo de itens numa página para assumir que existe uma próxima,
	/// quando o site não exibe link de paginação.
	pub page_size_hint: usize,
	/// User-Agent enviado em todas as requisições.
	pub user_agent: Cow<'static, str>,
}

impl Default for Params {
	fn default() -> Self {
		Self {
			base_url: "".into(),
			source_path: "manga".into(),
			page_size_hint: 10,
			user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".into(),
		}
	}
}

pub struct Madara<T: Impl> {
	inner: T,
	params: Params,
}

impl<T: Impl> Source for Madara<T> {
	fn new() -> Self {
		let inner = T::new();
		let params = inner.params();
		Self { inner, params }
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		self.inner
			.get_search_manga_list(&self.params, query, page, filters)
	}

	fn get_manga_update(
		&self,
		manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		self.inner
			.get_manga_update(&self.params, manga, needs_details, needs_chapters)
	}

	fn get_page_list(&self, manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		self.inner.get_page_list(&self.params, manga, chapter)
	}
}

impl<T: Impl> ListingProvider for Madara<T> {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		self.inner.get_manga_list(&self.params, listing, page)
	}
}

impl<T: Impl> DeepLinkHandler for Madara<T> {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		self.inner.handle_deep_link(&self.params, url)
	}
}
