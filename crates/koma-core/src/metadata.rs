use quick_xml::{de, se};
use serde::{Deserialize, Serialize};

use crate::{
    error::Result,
    model::{PublicationMetadata, ReadingDirection},
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename = "ComicInfo")]
pub struct ComicInfo {
    #[serde(rename = "Title", default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(rename = "Series", default, skip_serializing_if = "Option::is_none")]
    pub series: Option<String>,
    #[serde(rename = "Number", default, skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,
    #[serde(rename = "Volume", default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<i32>,
    #[serde(rename = "Summary", default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(rename = "Writer", default, skip_serializing_if = "Option::is_none")]
    pub writer: Option<String>,
    #[serde(rename = "Penciller", default, skip_serializing_if = "Option::is_none")]
    pub penciller: Option<String>,
    #[serde(rename = "Publisher", default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    #[serde(rename = "Genre", default, skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    #[serde(rename = "Tags", default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<String>,
    #[serde(rename = "Web", default, skip_serializing_if = "Option::is_none")]
    pub web: Option<String>,
    #[serde(
        rename = "LanguageISO",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub language_iso: Option<String>,
    #[serde(rename = "Manga", default, skip_serializing_if = "Option::is_none")]
    pub manga: Option<String>,
    #[serde(rename = "PageCount", default, skip_serializing_if = "Option::is_none")]
    pub page_count: Option<usize>,
}

impl ComicInfo {
    pub fn from_xml(xml: &str) -> Result<Self> {
        Ok(de::from_str(xml)?)
    }

    pub fn to_xml(&self) -> Result<String> {
        let body = se::to_string(self)?;
        Ok(format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n{body}"
        ))
    }

    pub fn merge_into(&self, metadata: &mut PublicationMetadata) {
        if let Some(value) = &self.title {
            metadata.title.clone_from(value);
        }
        metadata.series.clone_from(&self.series);
        metadata.number.clone_from(&self.number);
        metadata.volume = self.volume;
        metadata.summary.clone_from(&self.summary);
        metadata.writer.clone_from(&self.writer);
        metadata.penciller.clone_from(&self.penciller);
        metadata.publisher.clone_from(&self.publisher);
        metadata.language.clone_from(&self.language_iso);
        metadata.web.clone_from(&self.web);
        metadata.genres = split_list(self.genre.as_deref());
        metadata.tags = split_list(self.tags.as_deref());
        metadata.direction = match self.manga.as_deref().map(str::to_ascii_lowercase) {
            Some(value) if value == "yesandrighttoleft" || value == "yes" => {
                ReadingDirection::RightToLeft
            }
            _ => metadata.direction,
        };
    }

    pub fn from_metadata(metadata: &PublicationMetadata, page_count: usize) -> Self {
        Self {
            title: Some(metadata.title.clone()),
            series: metadata.series.clone(),
            number: metadata.number.clone(),
            volume: metadata.volume,
            summary: metadata.summary.clone(),
            writer: metadata.writer.clone(),
            penciller: metadata.penciller.clone(),
            publisher: metadata.publisher.clone(),
            genre: join_list(&metadata.genres),
            tags: join_list(&metadata.tags),
            web: metadata.web.clone(),
            language_iso: metadata.language.clone(),
            manga: (metadata.direction == ReadingDirection::RightToLeft)
                .then(|| "YesAndRightToLeft".to_owned()),
            page_count: Some(page_count),
        }
    }
}

fn split_list(value: Option<&str>) -> Vec<String> {
    value
        .into_iter()
        .flat_map(|value| value.split([',', ';']))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn join_list(values: &[String]) -> Option<String> {
    (!values.is_empty()).then(|| values.join(", "))
}

#[cfg(test)]
mod tests {
    use super::ComicInfo;

    #[test]
    fn round_trips_common_comic_info_fields() {
        let source = r#"<?xml version="1.0"?>
          <ComicInfo>
            <Title>Issue One</Title>
            <Series>Koma Test</Series>
            <Volume>1</Volume>
            <Manga>YesAndRightToLeft</Manga>
          </ComicInfo>"#;
        let info = ComicInfo::from_xml(source).expect("metadata parses");
        assert_eq!(info.title.as_deref(), Some("Issue One"));
        assert_eq!(info.volume, Some(1));
        let output = info.to_xml().expect("metadata serializes");
        assert!(output.contains("<Title>Issue One</Title>"));
    }
}
