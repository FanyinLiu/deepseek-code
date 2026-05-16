//! File tree sidebar for the TUI.
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use super::theme;

/// A node in the file tree.
#[derive(Debug, Clone)]
pub struct FileTreeNode {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub depth: usize,
    pub is_expanded: bool,
    pub children: Vec<FileTreeNode>,
}

/// The file tree state.
#[derive(Debug, Clone)]
pub struct FileTree {
    pub root: PathBuf,
    pub nodes: Vec<FileTreeNode>,
    pub mention_paths: Vec<String>,
    pub selected: usize,
    pub scroll_offset: usize,
}

impl FileTree {
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        let mut tree = Self {
            root: root.clone(),
            nodes: Vec::new(),
            mention_paths: Vec::new(),
            selected: 0,
            scroll_offset: 0,
        };
        tree.refresh();
        tree
    }

    pub fn refresh(&mut self) {
        self.nodes.clear();
        self.selected = 0;
        self.scroll_offset = 0;
        if let Ok(entries) = scan_directory(&self.root) {
            self.nodes = entries;
        }
        self.mention_paths = scan_mention_paths(&self.root).unwrap_or_default();
    }

    pub fn navigate_down(&mut self) {
        if self.selected + 1 < self.nodes.len() {
            self.selected += 1;
        }
    }

    pub fn navigate_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn toggle_expand(&mut self) {
        if let Some(node) = self.nodes.get_mut(self.selected) {
            if node.is_dir {
                node.is_expanded = !node.is_expanded;
                if node.is_expanded && node.children.is_empty() {
                    if let Ok(children) = scan_directory(&node.path) {
                        node.children = children;
                    }
                }
            }
        }
    }

    #[must_use]
    pub fn selected_path(&self) -> Option<PathBuf> {
        self.nodes.get(self.selected).map(|n| n.path.clone())
    }

    #[must_use]
    pub fn selected_is_dir(&self) -> bool {
        self.nodes
            .get(self.selected)
            .map(|n| n.is_dir)
            .unwrap_or(false)
    }

    #[must_use]
    pub fn flatten(&self) -> Vec<(&FileTreeNode, usize)> {
        let mut result = Vec::new();
        for node in &self.nodes {
            self.flatten_node(node, 0, &mut result);
        }
        result
    }

    fn flatten_node<'a>(
        &'a self,
        node: &'a FileTreeNode,
        depth: usize,
        result: &mut Vec<(&'a FileTreeNode, usize)>,
    ) {
        result.push((node, depth));
        if node.is_expanded {
            for child in &node.children {
                self.flatten_node(child, depth + 1, result);
            }
        }
    }
}

const MENTION_INDEX_LIMIT: usize = 20_000;

fn scan_directory(dir: &Path) -> Result<Vec<FileTreeNode>, anyhow::Error> {
    let mut nodes = Vec::new();
    let walker = WalkBuilder::new(dir)
        .max_depth(Some(1))
        .hidden(false)
        .git_ignore(true)
        .build();

    for entry in walker {
        let entry = entry?;
        let path = entry.path();
        if path == dir {
            continue;
        }
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let is_dir = path.is_dir();
        nodes.push(FileTreeNode {
            path: path.to_path_buf(),
            name,
            is_dir,
            depth: 0,
            is_expanded: false,
            children: Vec::new(),
        });
    }

    nodes.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    Ok(nodes)
}

fn scan_mention_paths(root: &Path) -> Result<Vec<String>, anyhow::Error> {
    let root_path = root.to_path_buf();
    let walker = WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .filter_entry(move |entry| !should_skip_mention_entry(entry.path(), &root_path))
        .build();

    let mut paths = Vec::new();
    for entry in walker {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        if path == root || !path.is_file() {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if rel.chars().any(char::is_whitespace) {
            continue;
        }
        paths.push(rel);
        if paths.len() >= MENTION_INDEX_LIMIT {
            break;
        }
    }
    paths.sort();
    Ok(paths)
}

fn should_skip_mention_entry(path: &Path, root: &Path) -> bool {
    if path == root {
        return false;
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            matches!(
                name,
                ".git" | "target" | "node_modules" | ".deepseek-code" | ".cache"
            )
        })
}

/// Render the file tree sidebar with clean card styling.
pub fn render_file_tree(f: &mut Frame, area: Rect, tree: &FileTree) {
    if area.width < 4 || area.height < 4 {
        return;
    }

    let palette = theme::palette();
    let block = Block::default()
        .title(Span::styled(
            " Files ",
            Style::default()
                .fg(palette.warning)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::border_style())
        .style(theme::style_card());

    let inner = block.inner(area);
    f.render_widget(block, area);

    let flat = tree.flatten();
    let mut lines = Vec::new();

    for (idx, (node, depth)) in flat.iter().enumerate() {
        let is_selected = idx == tree.selected;
        let indent = "  ".repeat(*depth);
        let (icon, icon_color) = if node.is_dir {
            if node.is_expanded {
                ("▾ ", palette.warning)
            } else {
                ("▸ ", palette.warning)
            }
        } else {
            ("  ", palette.secondary)
        };

        let _text = format!("{}{}{}", indent, icon, node.name);
        let style = if is_selected {
            Style::default()
                .fg(palette.text)
                .bg(palette.surface_alt)
                .add_modifier(Modifier::BOLD)
        } else if node.is_dir {
            Style::default().fg(palette.warning).bg(palette.surface)
        } else {
            Style::default().fg(palette.secondary).bg(palette.surface)
        };

        lines.push(Line::from(vec![
            Span::styled(indent, style),
            Span::styled(
                icon,
                Style::default()
                    .fg(icon_color)
                    .bg(style.bg.unwrap_or(palette.surface)),
            ),
            Span::styled(node.name.clone(), style),
        ]));
    }

    let visible = inner.height as usize;
    let start = tree.scroll_offset.min(lines.len().saturating_sub(1));
    let end = (start + visible).min(lines.len());
    let visible_lines = if start < end {
        lines[start..end].to_vec()
    } else {
        Vec::new()
    };

    let paragraph = Paragraph::new(Text::from(visible_lines)).style(theme::style_card());
    f.render_widget(paragraph, inner);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tree_navigation() {
        let mut tree = FileTree::new(PathBuf::from("."));
        tree.navigate_down();
        assert_eq!(tree.selected, 1);
        tree.navigate_up();
        assert_eq!(tree.selected, 0);
    }
}
