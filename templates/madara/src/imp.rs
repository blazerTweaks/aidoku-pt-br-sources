use crate::{
	Params,
	helpers::{
		absolute_url, ajax_post, image_url, parse_chapter_number, parse_date_text, parse_status,
		request, slug_from_url,
	},
};
use aidoku::{
	AidokuError, Chapter, DeepLinkResult, FilterValue, Listing, Manga, MangaPageResult, Page,
	PageContent, Result,
	alloc::{String, Vec, string::ToString},
	helpers::uri::encode_uri_component,
	imports::html::Document,
	prelude::*,
};

/// Comportamento de uma fonte Madara. Todos os métodos têm implementação padrão;
/// sobrescreva apenas o que for diferente no site.
pub trait Impl {
	fn new() -> Self;

	fn params(&self) -> Params;

	// -----------------------------------------------------------------------
	// URLs
	// -----------------------------------------------------------------------

	fn manga_url(&self, params: &Params, slug: &str) -> String {
		format!("{}/{}/{}/", params.base_url, params.source_path, slug)
	}

	fn search_url(&self, params: &Params, query: &str, page: i32) -> String {
		let page_path = if page > 1 { format!("/page/{}", page) } else { String::new() };
		format!(
			"{}{}/?s={}&post_type=wp-manga",
			params.base_url,
			page_path,
			encode_uri_component(query)
		)
	}

	/// Ordenação (`m_orderby`) usada por cada listing declarado no `source.json`.
	fn listing_order(&self, listing_id: &str) -> &'static str {
		match listing_id {
			"popular" => "views",
			"new" => "new-manga",
			"rating" => "rating",
			"trending" => "trending",
			"alphabet" => "alphabet",
			_ => "latest",
		}
	}

	fn listing_url(&self, params: &Params, listing_id: &str, page: i32) -> String {
		let page_path = if page > 1 { format!("page/{}/", page) } else { String::new() };
		format!(
			"{}/{}/{}?m_orderby={}",
			params.base_url,
			params.source_path,
			page_path,
			self.listing_order(listing_id)
		)
	}

	// -----------------------------------------------------------------------
	// Listas
	// -----------------------------------------------------------------------

	/// Itens das páginas de listagem (`.page-item-detail`) e de busca (`.c-tabs-item__content`).
	fn parse_manga_list(&self, params: &Params, html: &Document) -> Vec<Manga> {
		let mut entries: Vec<Manga> = Vec::new();
		let Some(items) = html.select(".page-item-detail, .c-tabs-item__content") else {
			return entries;
		};
		for item in items {
			let Some(link) = item.select_first(".item-thumb a, .tab-thumb a, .post-title a") else {
				continue;
			};
			let slug = slug_from_url(&link.attr("href").unwrap_or_default());
			if slug.is_empty() || entries.iter().any(|m| m.key == slug) {
				continue;
			}

			let title = item
				.select_first(".post-title h3 a, .post-title h4 a, .post-title a")
				.and_then(|e| e.text())
				.or_else(|| link.attr("title"))
				.unwrap_or_default();

			let cover = item
				.select_first(".item-thumb img, .tab-thumb img, img")
				.and_then(|e| image_url(params, &e));

			let status = item
				.select_first(".manga-title-badges, .mg_status .summary-content")
				.and_then(|e| e.text())
				.map(|s| parse_status(&s))
				.unwrap_or(aidoku::MangaStatus::Unknown);

			entries.push(Manga {
				url: Some(self.manga_url(params, &slug)),
				key: slug,
				title,
				cover,
				status,
				..Default::default()
			});
		}
		entries
	}

	fn has_next_page(&self, params: &Params, html: &Document, entries: &[Manga]) -> bool {
		html.select_first(".nav-previous a, a.nextpostslink, a.next.page-numbers, #navigation-ajax")
			.is_some()
			|| entries.len() >= params.page_size_hint
	}

	fn fetch_manga_list(&self, params: &Params, url: &str, page: i32) -> Result<MangaPageResult> {
		let html = match request(params, url)?.html() {
			Ok(html) => html,
			// Páginas além da última costumam devolver 404: encerra a paginação em vez de falhar.
			Err(_) if page > 1 => {
				return Ok(MangaPageResult {
					entries: Vec::new(),
					has_next_page: false,
				});
			}
			Err(e) => return Err(e.into()),
		};
		let entries = self.parse_manga_list(params, &html);
		let has_next_page = !entries.is_empty() && self.has_next_page(params, &html, &entries);
		Ok(MangaPageResult {
			entries,
			has_next_page,
		})
	}

	fn get_search_manga_list(
		&self,
		params: &Params,
		query: Option<String>,
		page: i32,
		_filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		match query.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
			Some(q) => {
				let url = self.search_url(params, q, page);
				self.fetch_manga_list(params, &url, page)
			}
			None => self.get_manga_list(
				params,
				Listing {
					id: "latest".into(),
					..Default::default()
				},
				page,
			),
		}
	}

	fn get_manga_list(&self, params: &Params, listing: Listing, page: i32) -> Result<MangaPageResult> {
		let url = self.listing_url(params, &listing.id, page);
		self.fetch_manga_list(params, &url, page)
	}

	// -----------------------------------------------------------------------
	// Detalhes e capítulos
	// -----------------------------------------------------------------------

	fn parse_details(&self, params: &Params, html: &Document, manga: &mut Manga) {
		if let Some(title) = html.select_first(".post-title h1").and_then(|e| e.text()) {
			manga.title = title;
		}

		if let Some(cover) = html.select_first(".summary_image img").and_then(|e| image_url(params, &e)) {
			manga.cover = Some(cover);
		}

		if let Some(desc) = html
			.select_first(".description-summary .summary__content, .summary__content, .manga-excerpt")
			.and_then(|e| e.text())
		{
			manga.description = Some(desc);
		}

		if let Some(authors) = html.select(".author-content a") {
			let authors: Vec<String> = authors.filter_map(|e| e.text()).collect();
			if !authors.is_empty() {
				manga.authors = Some(authors);
			}
		}

		if let Some(artists) = html.select(".artist-content a") {
			let artists: Vec<String> = artists.filter_map(|e| e.text()).collect();
			if !artists.is_empty() {
				manga.artists = Some(artists);
			}
		}

		if let Some(items) = html.select(".post-content .post-content_item, .post-status .post-content_item") {
			for item in items {
				let label = item
					.select_first(".summary-heading")
					.and_then(|e| e.text())
					.unwrap_or_default()
					.to_lowercase();
				if label.contains("status") {
					if let Some(status) = item.select_first(".summary-content").and_then(|e| e.text()) {
						manga.status = parse_status(&status);
					}
				}
			}
		}

		if let Some(genres) = html.select(".genres-content a") {
			let tags: Vec<String> = genres.filter_map(|e| e.text()).collect();
			if !tags.is_empty() {
				manga.tags = Some(tags);
			}
		}
	}

	/// Temas Madara recentes carregam a lista de capítulos via AJAX; tenta os dois endpoints conhecidos.
	fn load_chapter_document(&self, params: &Params, manga_url: &str, html: Document) -> Document {
		fn has_chapters(doc: &Document) -> bool {
			doc.select("li.wp-manga-chapter").is_some_and(|l| !l.is_empty())
		}

		if has_chapters(&html) {
			return html;
		}

		let url = format!("{}ajax/chapters/", manga_url);
		if let Ok(doc) = ajax_post(params, &url, manga_url).and_then(|r| r.html()) {
			if has_chapters(&doc) {
				return doc;
			}
		}

		let manga_id = html
			.select_first("#manga-chapters-holder")
			.and_then(|e| e.attr("data-id"))
			.or_else(|| html.select_first(".rating-post-id").and_then(|e| e.attr("value")));
		if let Some(id) = manga_id {
			let url = format!("{}/wp-admin/admin-ajax.php", params.base_url);
			let body = format!("action=manga_get_chapters&manga={}", id.trim());
			let legacy = ajax_post(params, &url, manga_url).map(|r| {
				r.header("Content-Type", "application/x-www-form-urlencoded; charset=UTF-8")
					.body(body.as_bytes())
			});
			if let Ok(doc) = legacy.and_then(|r| r.html()) {
				if has_chapters(&doc) {
					return doc;
				}
			}
		}

		html
	}

	fn parse_chapters(&self, params: &Params, doc: &Document) -> Vec<Chapter> {
		let mut chapters: Vec<Chapter> = Vec::new();
		let Some(items) = doc.select("li.wp-manga-chapter") else {
			return chapters;
		};
		for item in items {
			let Some(link) = item.select_first("a") else {
				continue;
			};
			let href = link.attr("href").map(|h| absolute_url(params, &h)).unwrap_or_default();
			if href.is_empty() {
				continue;
			}

			let title = item
				.select_first("span.chapternum")
				.and_then(|e| e.text())
				.or_else(|| link.text())
				.unwrap_or_default()
				.trim()
				.to_string();
			if title.is_empty() {
				continue;
			}

			let date_uploaded = item
				.select_first("span.chapter-release-date i, span.chapter-release-date, span.chapterdate")
				.and_then(|e| e.text())
				.and_then(|d| parse_date_text(&d))
				.or_else(|| {
					// Capítulos novos trazem a data só no `title` do selo "NEW".
					item.select_first("span.chapter-release-date a")
						.and_then(|e| e.attr("title"))
						.and_then(|d| parse_date_text(&d))
				});

			chapters.push(Chapter {
				key: href.clone(),
				chapter_number: parse_chapter_number(&title),
				title: Some(title),
				date_uploaded,
				url: Some(href),
				..Default::default()
			});
		}

		chapters.sort_by(|a, b| {
			b.chapter_number
				.unwrap_or(-1.0)
				.partial_cmp(&a.chapter_number.unwrap_or(-1.0))
				.unwrap_or(core::cmp::Ordering::Equal)
		});
		chapters
	}

	fn get_manga_update(
		&self,
		params: &Params,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let slug = if !manga.key.is_empty() {
			manga.key.clone()
		} else {
			manga.url.as_deref().map(slug_from_url).unwrap_or_default()
		};
		if slug.is_empty() {
			return Err(AidokuError::message("invalid slug"));
		}

		let url = self.manga_url(params, &slug);
		let html = request(params, &url)?.html()?;
		manga.url = Some(url.clone());

		if needs_details {
			self.parse_details(params, &html, &mut manga);
		}

		if needs_chapters {
			let doc = self.load_chapter_document(params, &url, html);
			manga.chapters = Some(self.parse_chapters(params, &doc));
		}

		Ok(manga)
	}

	// -----------------------------------------------------------------------
	// Páginas
	// -----------------------------------------------------------------------

	fn parse_pages(&self, params: &Params, html: &Document) -> Vec<Page> {
		let mut pages: Vec<Page> = Vec::new();
		if let Some(imgs) = html.select(".reading-content .page-break img, .reading-content img.wp-manga-chapter-img") {
			for img in imgs {
				if let Some(src) = image_url(params, &img) {
					if !pages.iter().any(|p| matches!(&p.content, PageContent::Url(u, _) if *u == src)) {
						pages.push(Page {
							content: PageContent::Url(src, None),
							..Default::default()
						});
					}
				}
			}
		}
		pages
	}

	fn get_page_list(&self, params: &Params, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		// `style=list` garante todas as imagens numa única página.
		let base = chapter.key.split('?').next().unwrap_or(&chapter.key);
		let url = format!("{}?style=list", base);
		let html = request(params, &url)?.html()?;
		let pages = self.parse_pages(params, &html);
		if pages.is_empty() {
			Err(AidokuError::message("no images found"))
		} else {
			Ok(pages)
		}
	}

	// -----------------------------------------------------------------------
	// Deep links
	// -----------------------------------------------------------------------

	fn handle_deep_link(&self, params: &Params, url: String) -> Result<Option<DeepLinkResult>> {
		let path = url.split(['?', '#']).next().unwrap_or_default();
		let marker = format!("/{}/", params.source_path);
		let Some(pos) = path.find(&marker) else {
			return Ok(None);
		};
		let segments: Vec<&str> = path[pos + marker.len()..]
			.split('/')
			.filter(|s| !s.is_empty())
			.collect();
		match segments.as_slice() {
			[slug] => Ok(Some(DeepLinkResult::Manga {
				key: slug.to_string(),
			})),
			[slug, chapter, ..] => Ok(Some(DeepLinkResult::Chapter {
				manga_key: slug.to_string(),
				key: format!("{}{}/", self.manga_url(params, slug), chapter),
			})),
			_ => Ok(None),
		}
	}
}
