//! Funções utilitárias do template Madara (independentes de site).
use crate::Params;
use aidoku::{
	MangaStatus,
	alloc::{String, Vec, string::ToString},
	imports::{
		html::Element,
		net::{Request, RequestError},
		std::{current_date, parse_date},
	},
	prelude::*,
};

/// GET com User-Agent e Referer do site.
pub fn request(params: &Params, url: &str) -> core::result::Result<Request, RequestError> {
	Ok(Request::get(url)?
		.header("User-Agent", params.user_agent.as_ref())
		.header("Referer", params.base_url.as_ref()))
}

/// POST no estilo XHR, usado pelos endpoints AJAX do Madara.
pub fn ajax_post(params: &Params, url: &str, referer: &str) -> core::result::Result<Request, RequestError> {
	Ok(Request::post(url)?
		.header("User-Agent", params.user_agent.as_ref())
		.header("Referer", referer)
		.header("X-Requested-With", "XMLHttpRequest"))
}

pub fn absolute_url(params: &Params, url: &str) -> String {
	let url = url.trim();
	if url.starts_with("//") {
		format!("https:{}", url)
	} else if url.starts_with('/') {
		format!("{}{}", params.base_url, url)
	} else {
		url.to_string()
	}
}

/// URL de uma imagem considerando lazy-load e os espaços/quebras de linha
/// que o tema Madara costuma deixar dentro do atributo.
pub fn image_url(params: &Params, el: &Element) -> Option<String> {
	for attr in ["data-src", "data-lazy-src", "data-cfsrc", "src"] {
		if let Some(value) = el.attr(attr) {
			let value = value.trim();
			if !value.is_empty() && !value.starts_with("data:") {
				return Some(absolute_url(params, value));
			}
		}
	}
	None
}

/// Último segmento do caminho, ignorando query, fragmento e barra final.
pub fn slug_from_url(url: &str) -> String {
	url.split(['?', '#'])
		.next()
		.unwrap_or_default()
		.trim_end_matches('/')
		.rsplit('/')
		.next()
		.unwrap_or_default()
		.to_string()
}

pub fn parse_status(text: &str) -> MangaStatus {
	let s = text.to_lowercase();
	if s.contains("complet") || s.contains("finaliz") || s.contains("conclu") {
		MangaStatus::Completed
	} else if s.contains("hiat") {
		MangaStatus::Hiatus
	} else if s.contains("cancel") {
		MangaStatus::Cancelled
	} else if s.contains("ongoing") || s.contains("andamento") || s.contains("em dia") || s.contains("lançamento") || s.contains("ativo") {
		MangaStatus::Ongoing
	} else {
		MangaStatus::Unknown
	}
}

/// Primeiro número (aceita `.` ou `,` decimal) encontrado no texto.
pub fn first_number(text: &str) -> Option<f32> {
	let mut num = String::new();
	for c in text.chars() {
		if c.is_ascii_digit() {
			num.push(c);
		} else if (c == '.' || c == ',') && !num.is_empty() && !num.contains('.') {
			num.push('.');
		} else if !num.is_empty() {
			break;
		}
	}
	num.trim_end_matches('.').parse::<f32>().ok()
}

/// Número do capítulo: prioriza o número logo após "capítulo"/"cap"/"chapter".
pub fn parse_chapter_number(text: &str) -> Option<f32> {
	let lower = text.to_lowercase();
	for key in ["capítulo", "capitulo", "cap.", "cap", "chapter", "ch."] {
		if let Some(pos) = lower.find(key) {
			if let Some(n) = first_number(&lower[pos + key.len()..]) {
				return Some(n);
			}
		}
	}
	first_number(&lower)
}

const MONTHS: [(&str, u32); 19] = [
	("jan", 1), ("fev", 2), ("feb", 2), ("mar", 3), ("abr", 4), ("apr", 4),
	("mai", 5), ("may", 5), ("jun", 6), ("jul", 7), ("ago", 8), ("aug", 8),
	("set", 9), ("sep", 9), ("out", 10), ("oct", 10), ("nov", 11), ("dez", 12), ("dec", 12),
];

/// Datas absolutas: "6 de janeiro de 2026", "janeiro 6, 2026", "06/01/2026", "2026-01-06".
fn parse_absolute_date(s: &str) -> Option<i64> {
	let mut month: Option<u32> = None;
	let mut numbers: Vec<&str> = Vec::new();
	for token in s.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()) {
		if token.chars().all(|c| c.is_ascii_digit()) {
			numbers.push(token);
		} else if month.is_none() {
			month = MONTHS
				.iter()
				.find(|(prefix, _)| token.starts_with(prefix))
				.map(|(_, m)| *m);
		}
	}

	let (y, m, d) = if let Some(m) = month {
		let year = numbers.iter().find(|n| n.len() == 4)?;
		let day = numbers.iter().find(|n| n.len() <= 2)?;
		(year.parse::<u32>().ok()?, m, day.parse::<u32>().ok()?)
	} else if numbers.len() >= 3 {
		if numbers[0].len() == 4 {
			(numbers[0].parse().ok()?, numbers[1].parse().ok()?, numbers[2].parse().ok()?)
		} else if numbers[2].len() == 4 {
			(numbers[2].parse().ok()?, numbers[1].parse().ok()?, numbers[0].parse().ok()?)
		} else {
			return None;
		}
	} else {
		return None;
	};

	if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
		return None;
	}
	parse_date(format!("{:04}-{:02}-{:02}", y, m, d), "yyyy-MM-dd")
}

/// Datas relativas: "há 2 dias", "3 horas atrás", "ontem", "2 days ago".
fn parse_relative_date(s: &str) -> Option<i64> {
	let now = current_date();
	if s.contains("hoje") || s.contains("agora") || s.contains("today") || s.contains("just now") {
		return Some(now);
	}
	if s.contains("ontem") || s.contains("yesterday") {
		return Some(now - 86_400);
	}
	let amount: i64 = match first_number(s) {
		Some(n) => n as i64,
		None if s.contains(" um ") || s.contains(" uma ") || s.starts_with("um ") || s.starts_with("uma ") || s.contains(" an ") || s.contains(" a ") => 1,
		None => return None,
	};
	let unit: i64 = if s.contains("seg") || s.contains("sec") {
		1
	} else if s.contains("min") {
		60
	} else if s.contains("hora") || s.contains("hour") {
		3_600
	} else if s.contains("dia") || s.contains("day") {
		86_400
	} else if s.contains("semana") || s.contains("week") {
		604_800
	} else if s.contains("mês") || s.contains("mes") || s.contains("month") {
		2_592_000
	} else if s.contains("ano") || s.contains("year") {
		31_536_000
	} else {
		return None;
	};
	Some(now - amount * unit)
}

/// Converte uma data absoluta ou relativa (pt-BR ou inglês) em timestamp Unix.
pub fn parse_date_text(text: &str) -> Option<i64> {
	let s = text.trim().to_lowercase();
	if s.is_empty() {
		return None;
	}
	parse_absolute_date(&s).or_else(|| parse_relative_date(&s))
}
