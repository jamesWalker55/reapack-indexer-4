use leon::{Template, Values};
use once_cell::sync::Lazy;

const REPOSITORY_STR: &str = include_str!("repository.ini");
const PACKAGE_STR: &str = include_str!("package.ini");
const VERSION_STR: &str = include_str!("version.ini");

static REPOSITORY_TEMPLATE: Lazy<Template> = Lazy::new(|| Template::parse(REPOSITORY_STR).unwrap());
static PACKAGE_TEMPLATE: Lazy<Template> = Lazy::new(|| Template::parse(PACKAGE_STR).unwrap());
static VERSION_TEMPLATE: Lazy<Template> = Lazy::new(|| Template::parse(VERSION_STR).unwrap());

pub(crate) struct RepositoryConfigParams<'a> {
    author: &'a str,
    url_pattern: &'a str,
    identifier: &'a str,
}

impl<'a> Values for RepositoryConfigParams<'a> {
    fn get_value(&self, key: &str) -> Option<std::borrow::Cow<'_, str>> {
        match key {
            "author" => Some(self.author.into()),
            "url_pattern" => Some(self.url_pattern.into()),
            "identifier" => Some(self.identifier.into()),
            _ => None,
        }
    }
}

impl<'a> RepositoryConfigParams<'a> {
    fn author(&mut self, val: &'a str) {
        self.author = val;
    }
    fn url_pattern(&mut self, val: &'a str) {
        self.url_pattern = val;
    }
    fn identifier(&mut self, val: &'a str) {
        self.identifier = val;
    }
}

impl<'a> Default for RepositoryConfigParams<'a> {
    fn default() -> Self {
        Self {
            author: "Your Name".into(),
            url_pattern: "https://raw.githubusercontent.com/YOUR_USERNAME/YOUR_REPOSITORY/{git_commit}/{relpath}".into(),
            identifier: "your-repository-identifier".into(),
        }
    }
}

fn generate_repository_config(params: &RepositoryConfigParams) -> String {
    let template = REPOSITORY_TEMPLATE.clone();
    template.render(&params).unwrap()
}
