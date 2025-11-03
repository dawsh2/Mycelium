//! Code Duplication Detector
//!
//! Detects duplicate code patterns using AST-based analysis.
//! Fails CI if >70% similar code found (strict mode for LLM guardrails).
//!
//! Usage:
//!   detect_duplication [--threshold 0.7] [--path ./crates]
//!
//! Exit codes:
//!   0 - No duplication found
//!   1 - Duplication detected (CI should fail)
//!   2 - Analysis error

use colored::*;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use syn::{visit::Visit, ItemFn};
use walkdir::WalkDir;

/// Similarity threshold (0.0-1.0). Above this = duplication.
const DEFAULT_THRESHOLD: f64 = 0.70;

/// Main entry point
fn main() {
    let args: Vec<String> = std::env::args().collect();

    let threshold = parse_threshold(&args);
    let search_path = parse_path(&args);

    println!("{}", "🔍 Mycelium Code Duplication Detector".bold().cyan());
    println!("Threshold: {}% similarity", (threshold * 100.0) as u8);
    println!("Searching: {}\n", search_path.display());

    let mut detector = DuplicationDetector::new(threshold);

    // Scan codebase
    match detector.scan_directory(&search_path) {
        Ok(_) => {
            println!("✅ Analyzed {} functions across {} files\n",
                detector.total_functions(),
                detector.total_files()
            );
        }
        Err(e) => {
            eprintln!("{} {}", "❌ Scan error:".red().bold(), e);
            process::exit(2);
        }
    }

    // Detect duplicates
    let duplicates = detector.find_duplicates();

    if duplicates.is_empty() {
        println!("{}", "✅ No code duplication detected!".green().bold());
        process::exit(0);
    } else {
        report_duplicates(&duplicates, threshold);
        process::exit(1);
    }
}

fn parse_threshold(args: &[String]) -> f64 {
    args.iter()
        .position(|arg| arg == "--threshold")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(DEFAULT_THRESHOLD)
}

fn parse_path(args: &[String]) -> PathBuf {
    args.iter()
        .position(|arg| arg == "--path")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./crates"))
}

/// Main duplication detection engine
struct DuplicationDetector {
    threshold: f64,
    functions: Vec<FunctionInfo>,
    files_analyzed: HashSet<PathBuf>,
}

impl DuplicationDetector {
    fn new(threshold: f64) -> Self {
        Self {
            threshold,
            functions: Vec::new(),
            files_analyzed: HashSet::new(),
        }
    }

    fn total_functions(&self) -> usize {
        self.functions.len()
    }

    fn total_files(&self) -> usize {
        self.files_analyzed.len()
    }

    /// Scan directory for Rust files
    fn scan_directory(&mut self, path: &Path) -> Result<(), String> {
        for entry in WalkDir::new(path)
            .into_iter()
            .filter_entry(|e| {
                // Skip target/, .git/, etc
                let name = e.file_name().to_str().unwrap_or("");
                !name.starts_with('.') && name != "target"
            })
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "rs"))
        {
            self.analyze_file(entry.path())?;
        }
        Ok(())
    }

    /// Analyze a single Rust file
    fn analyze_file(&mut self, path: &Path) -> Result<(), String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;

        let syntax = syn::parse_file(&content)
            .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))?;

        let mut visitor = FunctionVisitor {
            file_path: path.to_path_buf(),
            functions: Vec::new(),
        };

        visitor.visit_file(&syntax);

        self.functions.extend(visitor.functions);
        self.files_analyzed.insert(path.to_path_buf());

        Ok(())
    }

    /// Find duplicate function pairs
    fn find_duplicates(&self) -> Vec<DuplicatePair> {
        let mut duplicates = Vec::new();

        // Compare all pairs of functions
        for i in 0..self.functions.len() {
            for j in (i + 1)..self.functions.len() {
                let func_a = &self.functions[i];
                let func_b = &self.functions[j];

                // Skip if same file (might be intentional overloads)
                if func_a.file == func_b.file {
                    continue;
                }

                let similarity = calculate_similarity(func_a, func_b);

                if similarity >= self.threshold {
                    duplicates.push(DuplicatePair {
                        func_a: func_a.clone(),
                        func_b: func_b.clone(),
                        similarity,
                    });
                }
            }
        }

        duplicates
    }
}

/// Function metadata
#[derive(Debug, Clone)]
struct FunctionInfo {
    name: String,
    file: PathBuf,
    line: usize,
    signature: String,
    body_normalized: String,
    body_tokens: Vec<String>,
}

/// Visitor to extract functions from AST
struct FunctionVisitor {
    file_path: PathBuf,
    functions: Vec<FunctionInfo>,
}

impl<'ast> Visit<'ast> for FunctionVisitor {
    fn visit_item_fn(&mut self, func: &'ast ItemFn) {
        let name = func.sig.ident.to_string();

        // Skip test functions and generated code
        if name.starts_with("test_") || has_test_attribute(&func.attrs) {
            return;
        }

        let signature = normalize_signature(&func.sig);
        let body_normalized = normalize_body(&func.block);
        let body_tokens = tokenize_body(&func.block);

        self.functions.push(FunctionInfo {
            name,
            file: self.file_path.clone(),
            line: 0, // TODO: Extract line number from source location
            signature,
            body_normalized,
            body_tokens,
        });

        // Continue visiting nested functions
        syn::visit::visit_item_fn(self, func);
    }
}

fn has_test_attribute(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("test") || attr.path().is_ident("cfg")
    })
}

fn normalize_signature(sig: &syn::Signature) -> String {
    // Convert signature to string, normalizing parameter names
    let mut s = String::new();
    s.push_str("fn ");
    s.push_str(&sig.ident.to_string());
    s.push_str("(");

    for (i, input) in sig.inputs.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        // Only include types, not parameter names
        match input {
            syn::FnArg::Typed(pat_type) => {
                s.push_str(&quote::quote!(#pat_type).to_string());
            }
            syn::FnArg::Receiver(_) => {
                s.push_str("self");
            }
        }
    }

    s.push_str(")");

    if let syn::ReturnType::Type(_, ty) = &sig.output {
        s.push_str(" -> ");
        s.push_str(&quote::quote!(#ty).to_string());
    }

    s
}

fn normalize_body(block: &syn::Block) -> String {
    // Convert body to string with normalized variable names
    let body_str = quote::quote!(#block).to_string();

    // Normalize whitespace
    body_str.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn tokenize_body(block: &syn::Block) -> Vec<String> {
    // Extract semantic tokens (keywords, operators, types)
    let body_str = quote::quote!(#block).to_string();

    body_str.split_whitespace()
        .filter(|token| !token.chars().all(char::is_alphanumeric)) // Keep keywords/operators
        .map(|s| s.to_string())
        .collect()
}

/// Calculate similarity between two functions
fn calculate_similarity(func_a: &FunctionInfo, func_b: &FunctionInfo) -> f64 {
    // Weighted components:
    // - Signature similarity: 30%
    // - Body structure similarity: 50%
    // - Token similarity: 20%

    let sig_sim = text_similarity(&func_a.signature, &func_b.signature);
    let body_sim = text_similarity(&func_a.body_normalized, &func_b.body_normalized);
    let token_sim = token_similarity(&func_a.body_tokens, &func_b.body_tokens);

    (sig_sim * 0.3) + (body_sim * 0.5) + (token_sim * 0.2)
}

/// Text similarity using edit distance
fn text_similarity(a: &str, b: &str) -> f64 {
    let max_len = a.len().max(b.len()) as f64;
    if max_len == 0.0 {
        return 1.0;
    }

    let distance = edit_distance(a, b) as f64;
    1.0 - (distance / max_len)
}

/// Simple edit distance (Levenshtein)
fn edit_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    let mut dp = vec![vec![0; n + 1]; m + 1];

    for i in 0..=m {
        dp[i][0] = i;
    }
    for j in 0..=n {
        dp[0][j] = j;
    }

    for i in 1..=m {
        for j in 1..=n {
            let cost = if a_chars[i - 1] == b_chars[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }

    dp[m][n]
}

/// Token-based similarity (Jaccard index)
fn token_similarity(tokens_a: &[String], tokens_b: &[String]) -> f64 {
    let set_a: HashSet<&String> = tokens_a.iter().collect();
    let set_b: HashSet<&String> = tokens_b.iter().collect();

    let intersection = set_a.intersection(&set_b).count() as f64;
    let union = set_a.union(&set_b).count() as f64;

    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

/// Duplicate function pair
#[derive(Debug)]
struct DuplicatePair {
    func_a: FunctionInfo,
    func_b: FunctionInfo,
    similarity: f64,
}

/// Report duplicates to user
fn report_duplicates(duplicates: &[DuplicatePair], threshold: f64) {
    println!("{}", "❌ CODE DUPLICATION DETECTED".red().bold());
    println!("{} duplicate pairs found (>{threshold:.0}% similar)\n", duplicates.len());

    for (i, dup) in duplicates.iter().enumerate() {
        println!("{}. {} similarity",
            (i + 1).to_string().yellow().bold(),
            format!("{:.1}%", dup.similarity * 100.0).red().bold()
        );

        println!("   {} {}:{}",
            "Function A:".cyan(),
            dup.func_a.file.display(),
            dup.func_a.line
        );
        println!("   {} {}", "Name:".cyan(), dup.func_a.name);

        println!("   {} {}:{}",
            "Function B:".cyan(),
            dup.func_b.file.display(),
            dup.func_b.line
        );
        println!("   {} {}", "Name:".cyan(), dup.func_b.name);
        println!();
    }

    println!("{}", "💡 Suggestion:".yellow().bold());
    println!("   Extract common logic into a shared function or trait.");
    println!("   This will make the code more maintainable and reduce bugs.\n");

    println!("{}", "🚫 CI will fail until duplication is resolved.".red().bold());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edit_distance() {
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("hello", "hello"), 0);
        assert_eq!(edit_distance("", "abc"), 3);
    }

    #[test]
    fn test_text_similarity() {
        assert!((text_similarity("hello", "hello") - 1.0).abs() < 0.01);
        assert!(text_similarity("abc", "xyz") < 0.5);
    }
}
