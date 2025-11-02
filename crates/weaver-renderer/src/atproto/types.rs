use jacquard::types::string::CowStr;
use jacquard::types::blob::Blob;
use jacquard::smol_str::{SmolStrBuilder, ToSmolStr};

/// Blob name, validated to be URL-safe snake_case
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct BlobName<'a>(CowStr<'a>);

impl<'a> BlobName<'a> {
    /// Create blob name from filename, normalizing to lowercase snake_case
    pub fn from_filename(filename: &str) -> BlobName<'static> {
        let mut builder = SmolStrBuilder::new();
        for c in filename.chars() {
            if c.is_ascii_alphanumeric() {
                builder.push_str(&c.to_lowercase().to_smolstr());
            } else {
                builder.push_str("_");
            }
        }
        BlobName(CowStr::Owned(builder.finish()))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_ref()
    }
}

impl AsRef<str> for BlobName<'_> {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// Blob metadata tracked during preprocessing
#[derive(Debug, Clone)]
pub struct BlobInfo {
    pub name: BlobName<'static>,
    pub blob: Blob<'static>,
    pub alt: Option<CowStr<'static>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blob_name_normalization() {
        assert_eq!(BlobName::from_filename("My Image.PNG").as_str(), "my_image_png");
        assert_eq!(BlobName::from_filename("test-file!@#.jpg").as_str(), "test_file____jpg");
        assert_eq!(BlobName::from_filename("already_good").as_str(), "already_good");
        assert_eq!(BlobName::from_filename("CAPS").as_str(), "caps");
        assert_eq!(BlobName::from_filename("with spaces.txt").as_str(), "with_spaces_txt");
    }

    #[test]
    fn test_blob_name_hash_equality() {
        use std::collections::HashMap;

        let name1 = BlobName::from_filename("test.png");
        let name2 = BlobName::from_filename("test.png");

        let mut map = HashMap::new();
        map.insert(name1.clone(), "value");

        assert_eq!(map.get(&name2), Some(&"value"));
    }
}
