use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use regex::Regex;

pub type ShellResult<T> = Result<T, ShellError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellError {
    pub message: String,
    pub line: usize,
    pub column: usize,
}

impl ShellError {
    fn new(message: impl Into<String>, line: usize, column: usize) -> Self {
        Self {
            message: message.into(),
            line,
            column,
        }
    }
}

impl fmt::Display for ShellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "syntax error at {}:{}: {}",
            self.line, self.column, self.message
        )
    }
}

impl Error for ShellError {}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Script {
    pub statements: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    pub pipelines: Vec<Pipeline>,
    pub operators: Vec<ListOperator>,
    pub background: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListOperator {
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pipeline {
    pub commands: Vec<Command>,
    pub negated: bool,
    pub pipe_stderr: Vec<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Simple(SimpleCommand),
    If(IfCommand),
    For(ForCommand),
    ForArith(ForArithCommand),
    While(LoopCommand),
    Until(LoopCommand),
    Case(CaseCommand),
    FunctionDef(FunctionDef),
    Subshell(Vec<Statement>),
    Group(Vec<Statement>),
    Arithmetic(ArithmeticExpression),
    Conditional(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SimpleCommand {
    pub assignments: Vec<Assignment>,
    pub name: Option<Word>,
    pub args: Vec<Word>,
    pub redirections: Vec<Redirection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    pub name: String,
    pub value: Option<Word>,
    pub append: bool,
    pub array: Option<Vec<Word>>,
    pub index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redirection {
    pub fd: Option<u8>,
    pub operator: RedirectionOperator,
    pub target: Word,
    pub here_doc: Option<HereDoc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HereDoc {
    pub delimiter: String,
    pub content: String,
    pub quoted: bool,
    pub strip_tabs: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectionOperator {
    Input,
    Output,
    Append,
    DuplicateInput,
    DuplicateOutput,
    ReadWrite,
    Clobber,
    OutputBoth,
    AppendBoth,
    HereString,
    HereDoc,
    HereDocStripTabs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfCommand {
    pub clauses: Vec<IfClause>,
    pub else_body: Vec<Statement>,
    pub redirections: Vec<Redirection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfClause {
    pub condition: Vec<Statement>,
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForCommand {
    pub variable: String,
    pub words: Vec<Word>,
    /// Whether the loop had an explicit `in <list>` clause. When false
    /// (`for i; do ... done`) the loop iterates over the positional
    /// parameters instead of `words`.
    pub has_in_clause: bool,
    pub body: Vec<Statement>,
    pub redirections: Vec<Redirection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForArithCommand {
    pub init: String,
    pub condition: String,
    pub update: String,
    pub body: Vec<Statement>,
    pub redirections: Vec<Redirection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopCommand {
    pub condition: Vec<Statement>,
    pub body: Vec<Statement>,
    pub redirections: Vec<Redirection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseCommand {
    pub word: Word,
    pub items: Vec<CaseItem>,
    pub redirections: Vec<Redirection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaseTerminator {
    /// `;;` — stop after running this clause (default).
    #[default]
    Break,
    /// `;&` — fall through and run the next clause's body unconditionally.
    FallThrough,
    /// `;;&` — continue testing subsequent clause patterns.
    ContinueMatching,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseItem {
    pub patterns: Vec<Word>,
    pub body: Vec<Statement>,
    pub terminator: CaseTerminator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDef {
    pub name: String,
    pub body: Box<Command>,
    pub redirections: Vec<Redirection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Word {
    pub parts: Vec<WordPart>,
}

impl Word {
    pub fn literal(value: impl Into<String>) -> Self {
        Self {
            parts: vec![WordPart::Literal(value.into())],
        }
    }

    fn plain_text(&self) -> Option<String> {
        let mut text = String::new();
        for part in &self.parts {
            match part {
                WordPart::Literal(value) | WordPart::Escaped(value) => text.push_str(value),
                _ => return None,
            }
        }
        Some(text)
    }

    fn preserves_whitespace(&self) -> bool {
        self.parts.iter().any(WordPart::preserves_whitespace)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WordPart {
    Literal(String),
    SingleQuoted(String),
    DoubleQuoted(Vec<WordPart>),
    Escaped(String),
    Parameter(ParameterExpansion),
    CommandSubstitution { body: Script, legacy: bool },
    Arithmetic(ArithmeticExpression),
    Brace(BraceExpansion),
    Tilde { user: Option<String> },
}

impl WordPart {
    fn preserves_whitespace(&self) -> bool {
        matches!(
            self,
            Self::SingleQuoted(_) | Self::DoubleQuoted(_) | Self::Escaped(_)
        )
    }
}

fn here_doc_delimiter(word: &Word) -> Option<(String, bool)> {
    let mut delimiter = String::new();
    let mut quoted = false;
    for part in &word.parts {
        match part {
            WordPart::Literal(value) => delimiter.push_str(value),
            WordPart::Escaped(value) => {
                quoted = true;
                delimiter.push_str(value);
            }
            WordPart::SingleQuoted(value) => {
                quoted = true;
                delimiter.push_str(value);
            }
            WordPart::DoubleQuoted(parts) => {
                quoted = true;
                for part in parts {
                    match part {
                        WordPart::Literal(value)
                        | WordPart::Escaped(value)
                        | WordPart::SingleQuoted(value) => delimiter.push_str(value),
                        _ => return None,
                    }
                }
            }
            _ => return None,
        }
    }
    (!delimiter.is_empty()).then_some((delimiter, quoted))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterExpansion {
    pub parameter: String,
    pub operation: Option<ParameterOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParameterOperation {
    DefaultValue { word: Word, check_empty: bool },
    AssignDefault { word: Word, check_empty: bool },
    UseAlternative { word: Word, check_empty: bool },
    Length,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArithmeticExpression {
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BraceExpansion {
    pub items: Vec<BraceItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BraceItem {
    Word(Word),
    Range {
        start: String,
        end: String,
        step: Option<i64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Token {
    kind: TokenKind,
    line: usize,
    column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TokenKind {
    Word(Word),
    HereDocTarget {
        target: Word,
        here_doc: HereDoc,
    },
    Semicolon,
    Newline,
    DoubleSemicolon,
    /// `;&` — fall through to the next case clause's body unconditionally.
    SemicolonAmp,
    /// `;;&` — continue matching subsequent case patterns.
    DoubleSemicolonAmp,
    AndIf,
    OrIf,
    Pipe,
    PipeStderr,
    Amp,
    Bang,
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    Redirection(RedirectionOperator),
    ArithmeticCommand(ArithmeticExpression),
    ConditionalCommand(String),
    Eof,
}

/// Maximum accepted parser input size in bytes, mirroring upstream
/// `packages/just-bash/src/parser/types.ts` `MAX_INPUT_SIZE` (1 MB). Inputs
/// larger than this are rejected before tokenizing so the parser cannot be
/// driven into runaway compute.
pub const MAX_INPUT_SIZE: usize = 1_000_000;

/// Maximum number of lexer tokens the parser will accept, mirroring upstream
/// `MAX_TOKENS`. Token streams larger than this are rejected.
pub const MAX_TOKENS: usize = 100_000;

pub fn parse(input: &str) -> ShellResult<Script> {
    // Mirror upstream `Parser.parse`: reject oversized input before tokenizing.
    if input.len() > MAX_INPUT_SIZE {
        return Err(ShellError::new(
            format!(
                "Input too large: {} bytes exceeds limit of {MAX_INPUT_SIZE}",
                input.len()
            ),
            1,
            1,
        ));
    }
    let parser = Parser::new(input)?;
    // Mirror upstream token-count guard after tokenizing.
    if parser.tokens.len() > MAX_TOKENS {
        return Err(ShellError::new(
            format!(
                "Too many tokens: {} exceeds limit of {MAX_TOKENS}",
                parser.tokens.len()
            ),
            1,
            1,
        ));
    }
    parser.parse()
}

pub fn serialize(script: &Script) -> String {
    serialize_statements(&script.statements)
}

pub fn collect_command_names(script: &Script) -> Vec<String> {
    let mut names = std::collections::BTreeSet::new();
    collect_statements(&script.statements, &mut names);
    names.into_iter().collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TransformMetadata {
    pub commands: Vec<String>,
    pub tee_files: Vec<TeeFileInfo>,
    pub custom: BTreeMap<String, bool>,
}

impl TransformMetadata {
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty() && self.tee_files.is_empty() && self.custom.is_empty()
    }

    fn merge(&mut self, other: Self) {
        if !other.commands.is_empty() {
            self.commands = other.commands;
        }
        self.tee_files.extend(other.tee_files);
        self.custom.extend(other.custom);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeeFileInfo {
    pub command_index: usize,
    pub command_name: String,
    pub command: String,
    pub stdout_file: String,
}

pub struct TransformContext<'a> {
    pub ast: &'a Script,
    pub metadata: &'a TransformMetadata,
}

pub struct TransformResult {
    pub ast: Script,
    pub metadata: TransformMetadata,
}

pub trait TransformPlugin {
    fn name(&self) -> &str;
    fn transform(&mut self, context: TransformContext<'_>) -> ShellResult<TransformResult>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BashTransformResult {
    pub script: String,
    pub ast: Script,
    pub metadata: TransformMetadata,
}

#[derive(Default)]
pub struct BashTransformPipeline {
    plugins: Vec<Box<dyn TransformPlugin>>,
}

impl BashTransformPipeline {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn use_plugin(mut self, plugin: impl TransformPlugin + 'static) -> Self {
        self.plugins.push(Box::new(plugin));
        self
    }

    pub fn transform(&mut self, source: &str) -> ShellResult<BashTransformResult> {
        let mut ast = parse(source)?;
        let mut metadata = TransformMetadata::default();
        for plugin in &mut self.plugins {
            let result = plugin.transform(TransformContext {
                ast: &ast,
                metadata: &metadata,
            })?;
            ast = result.ast;
            metadata.merge(result.metadata);
        }
        Ok(BashTransformResult {
            script: serialize(&ast),
            ast,
            metadata,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct CommandCollectorPlugin;

impl TransformPlugin for CommandCollectorPlugin {
    fn name(&self) -> &str {
        "command-collector"
    }

    fn transform(&mut self, context: TransformContext<'_>) -> ShellResult<TransformResult> {
        Ok(TransformResult {
            ast: context.ast.clone(),
            metadata: TransformMetadata {
                commands: collect_command_names(context.ast),
                ..TransformMetadata::default()
            },
        })
    }
}

#[derive(Debug, Clone)]
pub struct TeePlugin {
    output_dir: String,
    timestamp: String,
    target_command_pattern: Option<Regex>,
    counter: usize,
}

impl TeePlugin {
    pub fn new(output_dir: impl Into<String>, timestamp: impl Into<String>) -> Self {
        Self {
            output_dir: output_dir.into(),
            timestamp: timestamp.into().replace(':', "-"),
            target_command_pattern: None,
            counter: 0,
        }
    }

    pub fn with_target_command_pattern(mut self, pattern: Regex) -> Self {
        self.target_command_pattern = Some(pattern);
        self
    }

    fn transform_script(&mut self, script: &Script, tee_files: &mut Vec<TeeFileInfo>) -> Script {
        Script {
            statements: script
                .statements
                .iter()
                .flat_map(|statement| self.transform_statement(statement, tee_files))
                .collect(),
        }
    }

    fn transform_statement(
        &mut self,
        statement: &Statement,
        tee_files: &mut Vec<TeeFileInfo>,
    ) -> Vec<Statement> {
        let mut transformed: Vec<Statement> = Vec::new();
        for (index, pipeline) in statement.pipelines.iter().enumerate() {
            let (pipeline, original_indices, negated) =
                self.transform_pipeline(pipeline, tee_files);
            let operator = index
                .checked_sub(1)
                .and_then(|idx| statement.operators.get(idx));
            if let Some(operator) = operator
                && let Some(previous) = transformed.last_mut()
            {
                previous.operators.push(*operator);
                previous.pipelines.push(pipeline);
            } else {
                transformed.push(Statement {
                    pipelines: vec![pipeline],
                    operators: Vec::new(),
                    background: statement.background && index + 1 == statement.pipelines.len(),
                });
            }

            if let Some(original_indices) = original_indices {
                transformed.push(Statement {
                    pipelines: vec![make_pipestatus_save(&original_indices)],
                    operators: Vec::new(),
                    background: false,
                });
                transformed.push(Statement {
                    pipelines: vec![make_pipestatus_restore(original_indices.len(), negated)],
                    operators: Vec::new(),
                    background: false,
                });
            }
        }
        transformed
    }

    fn transform_pipeline(
        &mut self,
        pipeline: &Pipeline,
        tee_files: &mut Vec<TeeFileInfo>,
    ) -> (Pipeline, Option<Vec<usize>>, bool) {
        if pipeline.commands.len() <= 1 {
            return (pipeline.clone(), None, false);
        }

        let mut commands = Vec::new();
        let mut pipe_stderr = Vec::new();
        let mut original_indices = Vec::new();
        let mut any_wrapped = false;

        for (index, command) in pipeline.commands.iter().enumerate() {
            let is_last = index + 1 == pipeline.commands.len();
            let Some(simple) = simple_command_for_tee(command) else {
                original_indices.push(commands.len());
                commands.push(command.clone());
                if !is_last {
                    pipe_stderr.push(pipeline.pipe_stderr.get(index).copied().unwrap_or_default());
                }
                continue;
            };

            if !self.should_target(simple) {
                original_indices.push(commands.len());
                commands.push(command.clone());
                if !is_last {
                    pipe_stderr.push(pipeline.pipe_stderr.get(index).copied().unwrap_or_default());
                }
                continue;
            }

            let command_name = extract_literal_command_name(simple.name.as_ref().expect("name"))
                .unwrap_or("unknown")
                .to_string();
            let command_index = self.counter;
            self.counter += 1;
            let stdout_file = format!(
                "{}/{}-{command_index:03}-{command_name}.stdout.txt",
                self.output_dir, self.timestamp
            );
            tee_files.push(TeeFileInfo {
                command_index,
                command_name,
                command: serialize_simple_command_without_redirections(simple),
                stdout_file: stdout_file.clone(),
            });

            original_indices.push(commands.len());
            commands.push(command.clone());
            pipe_stderr.push(pipeline.pipe_stderr.get(index).copied().unwrap_or_default());
            commands.push(Command::Simple(SimpleCommand {
                assignments: Vec::new(),
                name: Some(Word::literal("tee")),
                args: vec![Word::literal(stdout_file)],
                redirections: Vec::new(),
            }));
            if !is_last {
                pipe_stderr.push(false);
            }
            any_wrapped = true;
        }

        if !any_wrapped {
            return (pipeline.clone(), None, false);
        }

        (
            Pipeline {
                commands,
                negated: false,
                pipe_stderr,
            },
            Some(original_indices),
            pipeline.negated,
        )
    }

    fn should_target(&self, command: &SimpleCommand) -> bool {
        let Some(pattern) = &self.target_command_pattern else {
            return true;
        };
        command
            .name
            .as_ref()
            .and_then(extract_literal_command_name)
            .is_some_and(|name| pattern.is_match(name))
    }
}

impl TransformPlugin for TeePlugin {
    fn name(&self) -> &str {
        "tee"
    }

    fn transform(&mut self, context: TransformContext<'_>) -> ShellResult<TransformResult> {
        let mut tee_files = Vec::new();
        let ast = self.transform_script(context.ast, &mut tee_files);
        Ok(TransformResult {
            ast,
            metadata: TransformMetadata {
                tee_files,
                ..TransformMetadata::default()
            },
        })
    }
}

struct Lexer {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    column: usize,
    pending_here_doc_operator: Option<RedirectionOperator>,
    pending_here_docs: Vec<PendingHereDoc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingHereDoc {
    token_index: usize,
    delimiter: String,
    quoted: bool,
    strip_tabs: bool,
}

impl Lexer {
    fn new(input: &str) -> Self {
        Self {
            chars: input.chars().collect(),
            pos: 0,
            line: 1,
            column: 1,
            pending_here_doc_operator: None,
            pending_here_docs: Vec::new(),
        }
    }

    fn tokenize(mut self) -> ShellResult<Vec<Token>> {
        let mut tokens = Vec::new();
        while let Some(character) = self.peek() {
            if matches!(character, ' ' | '\t' | '\r') {
                self.advance();
                continue;
            }

            let line = self.line;
            let column = self.column;

            if character == '\n' {
                self.advance();
                tokens.push(Token {
                    kind: TokenKind::Newline,
                    line,
                    column,
                });
                self.read_pending_here_docs(&mut tokens, line, column)?;
                continue;
            }

            if character == '#' {
                self.skip_comment();
                continue;
            }

            if self.starts_with("((") {
                let expression = self.read_arithmetic_command(line, column)?;
                tokens.push(Token {
                    kind: TokenKind::ArithmeticCommand(ArithmeticExpression { source: expression }),
                    line,
                    column,
                });
                continue;
            }

            if self.starts_with("[[") {
                let expression = self.read_conditional_command(line, column)?;
                tokens.push(Token {
                    kind: TokenKind::ConditionalCommand(expression),
                    line,
                    column,
                });
                continue;
            }

            if let Some(kind) = self.read_operator() {
                if let TokenKind::Redirection(operator) = &kind
                    && matches!(
                        operator,
                        RedirectionOperator::HereDoc | RedirectionOperator::HereDocStripTabs
                    )
                {
                    self.pending_here_doc_operator = Some(*operator);
                }
                tokens.push(Token { kind, line, column });
                continue;
            }

            let word = self.read_word(line, column)?;
            let pending_here_doc_operator = self.pending_here_doc_operator.take();
            let token_index = tokens.len();
            tokens.push(Token {
                kind: TokenKind::Word(word),
                line,
                column,
            });
            if let Some(operator) = pending_here_doc_operator {
                let TokenKind::Word(word) = &tokens[token_index].kind else {
                    unreachable!("just pushed a word token");
                };
                let (delimiter, quoted) = here_doc_delimiter(word).ok_or_else(|| {
                    ShellError::new("invalid here-document delimiter", line, column)
                })?;
                self.pending_here_docs.push(PendingHereDoc {
                    token_index,
                    delimiter,
                    quoted,
                    strip_tabs: operator == RedirectionOperator::HereDocStripTabs,
                });
            }
        }

        if !self.pending_here_docs.is_empty() {
            self.read_pending_here_docs(&mut tokens, self.line, self.column)?;
        }

        tokens.push(Token {
            kind: TokenKind::Eof,
            line: self.line,
            column: self.column,
        });
        Ok(tokens)
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_offset(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn starts_with(&self, pattern: &str) -> bool {
        pattern
            .chars()
            .enumerate()
            .all(|(offset, character)| self.peek_offset(offset) == Some(character))
    }

    fn advance(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.pos += 1;
        if character == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(character)
    }

    fn take(&mut self, count: usize) {
        for _ in 0..count {
            self.advance();
        }
    }

    fn skip_comment(&mut self) {
        while let Some(character) = self.peek() {
            if character == '\n' {
                break;
            }
            self.advance();
        }
    }

    fn read_pending_here_docs(
        &mut self,
        tokens: &mut [Token],
        line: usize,
        column: usize,
    ) -> ShellResult<()> {
        let pending = std::mem::take(&mut self.pending_here_docs);
        for here_doc in pending {
            let content = self.read_here_doc_content(&here_doc, line, column)?;
            let target = match tokens.get(here_doc.token_index).map(|token| &token.kind) {
                Some(TokenKind::Word(_)) => Word::literal(here_doc.delimiter.clone()),
                _ => {
                    return Err(ShellError::new(
                        "missing here-document target",
                        line,
                        column,
                    ));
                }
            };
            tokens[here_doc.token_index].kind = TokenKind::HereDocTarget {
                target,
                here_doc: HereDoc {
                    delimiter: here_doc.delimiter,
                    content,
                    quoted: here_doc.quoted,
                    strip_tabs: here_doc.strip_tabs,
                },
            };
        }
        Ok(())
    }

    fn read_here_doc_content(
        &mut self,
        here_doc: &PendingHereDoc,
        line: usize,
        column: usize,
    ) -> ShellResult<String> {
        let mut content = String::new();
        let mut line_buffer = String::new();

        while let Some(character) = self.peek() {
            self.advance();
            line_buffer.push(character);

            if character != '\n' {
                continue;
            }

            let line_without_newline = line_buffer.strip_suffix('\n').unwrap_or(&line_buffer);
            let delimiter_candidate = if here_doc.strip_tabs {
                line_without_newline.trim_start_matches('\t')
            } else {
                line_without_newline
            };
            if delimiter_candidate == here_doc.delimiter {
                return Ok(content);
            }

            content.push_str(&line_buffer);
            line_buffer.clear();
        }

        if !line_buffer.is_empty() {
            let delimiter_candidate = if here_doc.strip_tabs {
                line_buffer.trim_start_matches('\t')
            } else {
                line_buffer.as_str()
            };
            if delimiter_candidate == here_doc.delimiter {
                return Ok(content);
            }
            content.push_str(&line_buffer);
        }

        Err(ShellError::new(
            format!(
                "here-document delimited by end-of-file: {}",
                here_doc.delimiter
            ),
            line,
            column,
        ))
    }

    fn read_operator(&mut self) -> Option<TokenKind> {
        for (text, kind) in [
            (
                "&>>",
                TokenKind::Redirection(RedirectionOperator::AppendBoth),
            ),
            (
                "<<-",
                TokenKind::Redirection(RedirectionOperator::HereDocStripTabs),
            ),
            (
                "<<<",
                TokenKind::Redirection(RedirectionOperator::HereString),
            ),
            ("&&", TokenKind::AndIf),
            ("||", TokenKind::OrIf),
            ("|&", TokenKind::PipeStderr),
            (";;&", TokenKind::DoubleSemicolonAmp),
            (";&", TokenKind::SemicolonAmp),
            (";;", TokenKind::DoubleSemicolon),
            ("<<", TokenKind::Redirection(RedirectionOperator::HereDoc)),
            (">>", TokenKind::Redirection(RedirectionOperator::Append)),
            (
                "<&",
                TokenKind::Redirection(RedirectionOperator::DuplicateInput),
            ),
            (
                ">&",
                TokenKind::Redirection(RedirectionOperator::DuplicateOutput),
            ),
            ("<>", TokenKind::Redirection(RedirectionOperator::ReadWrite)),
            (">|", TokenKind::Redirection(RedirectionOperator::Clobber)),
            (
                "&>",
                TokenKind::Redirection(RedirectionOperator::OutputBoth),
            ),
        ] {
            if self.starts_with(text) {
                self.take(text.chars().count());
                return Some(kind);
            }
        }

        let kind = match self.peek()? {
            ';' => TokenKind::Semicolon,
            '|' => TokenKind::Pipe,
            '&' => TokenKind::Amp,
            '!' => TokenKind::Bang,
            '(' => TokenKind::LeftParen,
            ')' => TokenKind::RightParen,
            '{' if self
                .peek_offset(1)
                .is_none_or(Self::is_separator_after_group_brace) =>
            {
                TokenKind::LeftBrace
            }
            '}' => TokenKind::RightBrace,
            '<' => TokenKind::Redirection(RedirectionOperator::Input),
            '>' => TokenKind::Redirection(RedirectionOperator::Output),
            _ => return None,
        };
        self.advance();
        Some(kind)
    }

    fn is_separator_after_group_brace(character: char) -> bool {
        character.is_whitespace() || matches!(character, ';' | '&' | '|' | ')' | '}')
    }

    fn read_arithmetic_command(&mut self, line: usize, column: usize) -> ShellResult<String> {
        self.take(2);
        let mut expression = String::new();
        let mut depth = 0usize;
        while let Some(character) = self.peek() {
            if character == '(' {
                depth += 1;
                expression.push(character);
                self.advance();
                continue;
            }
            if character == ')' {
                if depth == 0 && self.peek_offset(1) == Some(')') {
                    self.take(2);
                    return Ok(expression.trim().to_string());
                }
                depth = depth.saturating_sub(1);
                expression.push(character);
                self.advance();
                continue;
            }
            expression.push(character);
            self.advance();
        }
        Err(ShellError::new("unclosed arithmetic command", line, column))
    }

    fn read_conditional_command(&mut self, line: usize, column: usize) -> ShellResult<String> {
        self.take(2);
        let mut expression = String::new();
        while let Some(character) = self.peek() {
            if character == ']' && self.peek_offset(1) == Some(']') {
                self.take(2);
                return Ok(expression.trim().to_string());
            }
            expression.push(character);
            self.advance();
        }
        Err(ShellError::new(
            "unclosed conditional command",
            line,
            column,
        ))
    }

    fn read_word(&mut self, line: usize, column: usize) -> ShellResult<Word> {
        let mut parts = Vec::new();
        while let Some(character) = self.peek() {
            if self.is_word_boundary() {
                break;
            }

            match character {
                '\'' => parts.push(WordPart::SingleQuoted(
                    self.read_single_quoted(line, column)?,
                )),
                '"' => parts.push(WordPart::DoubleQuoted(
                    self.read_double_quoted(line, column)?,
                )),
                '\\' => parts.push(WordPart::Escaped(self.read_escape())),
                '`' => parts.push(WordPart::CommandSubstitution {
                    body: self.read_legacy_substitution(line, column)?,
                    legacy: true,
                }),
                '$' => parts.push(self.read_dollar_expansion(line, column)?),
                '~' if parts.is_empty() => {
                    parts.push(WordPart::Tilde {
                        user: self.read_tilde_user(),
                    });
                }
                '{' if self.brace_expansion_end().is_some() => {
                    parts.push(WordPart::Brace(self.read_brace_expansion(line, column)?));
                }
                _ => parts.push(WordPart::Literal(self.read_literal())),
            }
        }

        if parts.is_empty() {
            Err(ShellError::new("expected word", line, column))
        } else {
            Ok(Word { parts })
        }
    }

    fn is_word_boundary(&self) -> bool {
        let Some(character) = self.peek() else {
            return true;
        };
        character.is_whitespace()
            || matches!(
                character,
                ';' | '&' | '|' | '<' | '>' | '(' | ')' | '}' | '!'
            )
            || self.starts_with("[[")
            || (character == '{'
                && self
                    .peek_offset(1)
                    .is_none_or(Self::is_separator_after_group_brace))
    }

    fn read_literal(&mut self) -> String {
        let mut value = String::new();
        while let Some(character) = self.peek() {
            if self.is_word_boundary()
                || matches!(character, '\'' | '"' | '\\' | '$' | '`')
                || (character == '{' && self.brace_expansion_end().is_some())
            {
                break;
            }
            value.push(character);
            self.advance();
        }
        value
    }

    fn read_single_quoted(&mut self, line: usize, column: usize) -> ShellResult<String> {
        self.advance();
        let mut value = String::new();
        while let Some(character) = self.peek() {
            self.advance();
            if character == '\'' {
                return Ok(value);
            }
            value.push(character);
        }
        Err(ShellError::new("unclosed single quote", line, column))
    }

    fn read_double_quoted(&mut self, line: usize, column: usize) -> ShellResult<Vec<WordPart>> {
        self.advance();
        let mut parts = Vec::new();
        let mut literal = String::new();
        while let Some(character) = self.peek() {
            match character {
                '"' => {
                    self.advance();
                    if !literal.is_empty() {
                        parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                    }
                    return Ok(parts);
                }
                '\\' => {
                    self.advance();
                    if let Some(escaped) = self.peek() {
                        self.advance();
                        let value = match escaped {
                            '$' | '"' | '\\' | '`' | '\n' => escaped.to_string(),
                            other => {
                                let mut text = String::from("\\");
                                text.push(other);
                                text
                            }
                        };
                        literal.push_str(&value);
                    } else {
                        literal.push('\\');
                    }
                }
                '$' => {
                    if !literal.is_empty() {
                        parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                    }
                    parts.push(self.read_dollar_expansion(line, column)?);
                }
                '`' => {
                    if !literal.is_empty() {
                        parts.push(WordPart::Literal(std::mem::take(&mut literal)));
                    }
                    parts.push(WordPart::CommandSubstitution {
                        body: self.read_legacy_substitution(line, column)?,
                        legacy: true,
                    });
                }
                _ => {
                    literal.push(character);
                    self.advance();
                }
            }
        }
        Err(ShellError::new("unclosed double quote", line, column))
    }

    fn read_escape(&mut self) -> String {
        self.advance();
        match self.advance() {
            Some(character) => character.to_string(),
            None => "\\".to_string(),
        }
    }

    fn read_tilde_user(&mut self) -> Option<String> {
        self.advance();
        let mut user = String::new();
        while let Some(character) = self.peek() {
            if character == '/' || self.is_word_boundary() {
                break;
            }
            user.push(character);
            self.advance();
        }
        (!user.is_empty()).then_some(user)
    }

    fn read_dollar_expansion(&mut self, line: usize, column: usize) -> ShellResult<WordPart> {
        self.advance();
        if self.starts_with("((") {
            self.take(2);
            let source = self.read_until_arithmetic_expansion_end(line, column)?;
            return Ok(WordPart::Arithmetic(ArithmeticExpression { source }));
        }

        if self.peek() == Some('(') {
            self.advance();
            let body = self.read_command_substitution_body(line, column)?;
            return Ok(WordPart::CommandSubstitution {
                body: parse(&body)?,
                legacy: false,
            });
        }

        if self.peek() == Some('{') {
            self.advance();
            let content = self.read_braced_parameter_content(line, column)?;
            return Ok(WordPart::Parameter(parse_parameter_expansion(&content)));
        }

        let parameter = match self.peek() {
            Some(character) if is_special_parameter(character) => {
                self.advance();
                character.to_string()
            }
            Some(character) if is_name_start(character) => self.read_name(),
            _ => "$".to_string(),
        };

        if parameter == "$" {
            Ok(WordPart::Literal(parameter))
        } else {
            Ok(WordPart::Parameter(ParameterExpansion {
                parameter,
                operation: None,
            }))
        }
    }

    fn read_until_arithmetic_expansion_end(
        &mut self,
        line: usize,
        column: usize,
    ) -> ShellResult<String> {
        let mut expression = String::new();
        let mut depth = 0usize;
        while let Some(character) = self.peek() {
            if character == '(' {
                depth += 1;
                expression.push(character);
                self.advance();
                continue;
            }
            if character == ')' {
                if depth == 0 && self.peek_offset(1) == Some(')') {
                    self.take(2);
                    return Ok(expression.trim().to_string());
                }
                depth = depth.saturating_sub(1);
                expression.push(character);
                self.advance();
                continue;
            }
            expression.push(character);
            self.advance();
        }
        Err(ShellError::new(
            "unclosed arithmetic expansion",
            line,
            column,
        ))
    }

    fn read_command_substitution_body(
        &mut self,
        line: usize,
        column: usize,
    ) -> ShellResult<String> {
        let mut body = String::new();
        let mut depth = 0usize;
        let mut quote: Option<char> = None;
        while let Some(character) = self.peek() {
            if let Some(quote_char) = quote {
                body.push(character);
                self.advance();
                if character == '\\' {
                    if let Some(next) = self.peek() {
                        body.push(next);
                        self.advance();
                    }
                } else if character == quote_char {
                    quote = None;
                }
                continue;
            }

            match character {
                '\'' | '"' => {
                    quote = Some(character);
                    body.push(character);
                    self.advance();
                }
                '(' => {
                    depth += 1;
                    body.push(character);
                    self.advance();
                }
                ')' if depth == 0 => {
                    self.advance();
                    return Ok(body);
                }
                ')' => {
                    depth -= 1;
                    body.push(character);
                    self.advance();
                }
                _ => {
                    body.push(character);
                    self.advance();
                }
            }
        }
        Err(ShellError::new(
            "unclosed command substitution",
            line,
            column,
        ))
    }

    fn read_legacy_substitution(&mut self, line: usize, column: usize) -> ShellResult<Script> {
        self.advance();
        let mut body = String::new();
        while let Some(character) = self.peek() {
            self.advance();
            if character == '`' {
                return parse(&body);
            }
            body.push(character);
        }
        Err(ShellError::new(
            "unclosed backtick substitution",
            line,
            column,
        ))
    }

    fn read_braced_parameter_content(&mut self, line: usize, column: usize) -> ShellResult<String> {
        let mut content = String::new();
        let mut depth = 0usize;
        while let Some(character) = self.peek() {
            if character == '{' {
                depth += 1;
                content.push(character);
                self.advance();
                continue;
            }
            if character == '}' {
                if depth == 0 {
                    self.advance();
                    return Ok(content);
                }
                depth -= 1;
                content.push(character);
                self.advance();
                continue;
            }
            content.push(character);
            self.advance();
        }
        Err(ShellError::new(
            "unclosed parameter expansion",
            line,
            column,
        ))
    }

    fn read_name(&mut self) -> String {
        let mut name = String::new();
        while let Some(character) = self.peek() {
            if is_name_continue(character) {
                name.push(character);
                self.advance();
            } else {
                break;
            }
        }
        name
    }

    fn brace_expansion_end(&self) -> Option<usize> {
        if self.peek() != Some('{') {
            return None;
        }
        let mut depth = 0usize;
        let mut saw_comma = false;
        let mut saw_range = false;
        let mut index = self.pos;
        while let Some(character) = self.chars.get(index).copied() {
            match character {
                '{' => depth += 1,
                '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return (saw_comma || saw_range).then_some(index);
                    }
                }
                ',' if depth == 1 => saw_comma = true,
                '.' if depth == 1 && self.chars.get(index + 1) == Some(&'.') => saw_range = true,
                _ => {}
            }
            index += 1;
        }
        None
    }

    fn read_brace_expansion(&mut self, line: usize, column: usize) -> ShellResult<BraceExpansion> {
        let end = self
            .brace_expansion_end()
            .ok_or_else(|| ShellError::new("invalid brace expansion", line, column))?;
        self.advance();
        let content: String = self.chars[self.pos..end].iter().collect();
        while self.pos <= end {
            self.advance();
        }
        Ok(parse_brace_items(&content))
    }
}

fn parse_parameter_expansion(content: &str) -> ParameterExpansion {
    if let Some(parameter) = content.strip_prefix('#') {
        return ParameterExpansion {
            parameter: parameter.to_string(),
            operation: Some(ParameterOperation::Length),
        };
    }

    for (operator, check_empty, kind) in [
        (":-", true, 0u8),
        ("-", false, 0),
        (":=", true, 1),
        ("=", false, 1),
        (":+", true, 2),
        ("+", false, 2),
    ] {
        if let Some(index) = content.find(operator) {
            let parameter = content[..index].to_string();
            let word = Word::literal(content[index + operator.len()..].to_string());
            let operation = match kind {
                0 => ParameterOperation::DefaultValue { word, check_empty },
                1 => ParameterOperation::AssignDefault { word, check_empty },
                _ => ParameterOperation::UseAlternative { word, check_empty },
            };
            return ParameterExpansion {
                parameter,
                operation: Some(operation),
            };
        }
    }

    ParameterExpansion {
        parameter: content.to_string(),
        operation: None,
    }
}

fn parse_brace_items(content: &str) -> BraceExpansion {
    let range_parts = content.split("..").collect::<Vec<_>>();
    if (2..=3).contains(&range_parts.len()) {
        let step = range_parts.get(2).and_then(|part| part.parse::<i64>().ok());
        return BraceExpansion {
            items: vec![BraceItem::Range {
                start: range_parts[0].to_string(),
                end: range_parts[1].to_string(),
                step,
            }],
        };
    }

    BraceExpansion {
        items: split_top_level(content, ',')
            .into_iter()
            .map(|item| BraceItem::Word(Word::literal(item)))
            .collect(),
    }
}

fn split_top_level(content: &str, separator: char) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    for character in content.chars() {
        match character {
            '{' => {
                depth += 1;
                current.push(character);
            }
            '}' => {
                depth = depth.saturating_sub(1);
                current.push(character);
            }
            item_separator if item_separator == separator && depth == 0 => {
                items.push(std::mem::take(&mut current));
            }
            _ => current.push(character),
        }
    }
    items.push(current);
    items
}

fn serialize_statements(statements: &[Statement]) -> String {
    statements
        .iter()
        .map(serialize_statement)
        .collect::<Vec<_>>()
        .join("\n")
}

fn serialize_inline_statements(statements: &[Statement]) -> String {
    statements
        .iter()
        .map(serialize_statement)
        .collect::<Vec<_>>()
        .join("; ")
}

fn serialize_statement(statement: &Statement) -> String {
    let mut here_docs = Vec::new();
    let mut output = serialize_pipeline(&statement.pipelines[0], &mut here_docs);
    for (operator, pipeline) in statement
        .operators
        .iter()
        .zip(statement.pipelines.iter().skip(1))
    {
        output.push_str(match operator {
            ListOperator::And => " && ",
            ListOperator::Or => " || ",
        });
        output.push_str(&serialize_pipeline(pipeline, &mut here_docs));
    }
    if statement.background {
        if !output.is_empty() {
            output.push(' ');
        }
        output.push('&');
    }
    if !here_docs.is_empty() {
        output.push('\n');
        output.push_str(&here_docs.join("\n"));
    }
    output
}

fn serialize_pipeline(pipeline: &Pipeline, here_docs: &mut Vec<String>) -> String {
    let mut output = String::new();
    if pipeline.negated {
        output.push_str("! ");
    }
    for (index, command) in pipeline.commands.iter().enumerate() {
        if index > 0 {
            output.push_str(
                if pipeline
                    .pipe_stderr
                    .get(index - 1)
                    .copied()
                    .unwrap_or_default()
                {
                    " |& "
                } else {
                    " | "
                },
            );
        }
        output.push_str(&serialize_command(command, here_docs));
    }
    output
}

fn serialize_command(command: &Command, here_docs: &mut Vec<String>) -> String {
    match command {
        Command::Simple(command) => serialize_simple_command(command, here_docs),
        Command::If(command) => serialize_if(command),
        Command::For(command) => serialize_for(command),
        Command::ForArith(command) => serialize_for_arith(command),
        Command::While(command) => serialize_loop("while", command),
        Command::Until(command) => serialize_loop("until", command),
        Command::Case(command) => serialize_case(command),
        Command::FunctionDef(function) => format!(
            "{}() {}{}",
            function.name,
            serialize_command(&function.body, here_docs),
            serialize_redirections(&function.redirections)
        ),
        Command::Subshell(body) => format!("({})", serialize_inline_statements(body)),
        Command::Group(body) => {
            let body = serialize_inline_statements(body);
            if body.is_empty() {
                "{}".to_string()
            } else {
                format!("{{ {body}; }}")
            }
        }
        Command::Arithmetic(expression) => format!("(({}))", expression.source),
        Command::Conditional(expression) => format!("[[ {} ]]", expression),
    }
}

fn serialize_simple_command(command: &SimpleCommand, here_docs: &mut Vec<String>) -> String {
    let mut parts = Vec::new();
    parts.extend(command.assignments.iter().map(serialize_assignment));
    if let Some(name) = &command.name {
        parts.push(serialize_word(name));
    }
    parts.extend(command.args.iter().map(serialize_word));
    parts.extend(
        command
            .redirections
            .iter()
            .map(|redirection| serialize_redirection(redirection, here_docs)),
    );
    parts.join(" ")
}

fn serialize_assignment(assignment: &Assignment) -> String {
    let mut target = assignment.name.clone();
    if let Some(index) = assignment.index {
        target.push('[');
        target.push_str(&index.to_string());
        target.push(']');
    }
    if assignment.append {
        target.push('+');
    }
    target.push('=');
    if let Some(array) = &assignment.array {
        target.push('(');
        target.push_str(
            &array
                .iter()
                .map(serialize_word)
                .collect::<Vec<_>>()
                .join(" "),
        );
        target.push(')');
    } else if let Some(value) = &assignment.value {
        target.push_str(&serialize_word(value));
    }
    target
}

fn serialize_if(command: &IfCommand) -> String {
    let mut output = String::new();
    for (index, clause) in command.clauses.iter().enumerate() {
        if index == 0 {
            output.push_str("if ");
        } else {
            output.push_str("; elif ");
        }
        output.push_str(&serialize_inline_statements(&clause.condition));
        output.push_str("; then ");
        output.push_str(&serialize_inline_statements(&clause.body));
    }
    if !command.else_body.is_empty() {
        output.push_str("; else ");
        output.push_str(&serialize_inline_statements(&command.else_body));
    }
    output.push_str("; fi");
    output.push_str(&serialize_redirections(&command.redirections));
    output
}

fn serialize_for(command: &ForCommand) -> String {
    let mut output = format!("for {}", command.variable);
    if command.words.is_empty() {
        output.push_str("; do ");
    } else {
        output.push_str(" in ");
        output.push_str(
            &command
                .words
                .iter()
                .map(serialize_word)
                .collect::<Vec<_>>()
                .join(" "),
        );
        output.push_str("; do ");
    }
    output.push_str(&serialize_inline_statements(&command.body));
    output.push_str("; done");
    output.push_str(&serialize_redirections(&command.redirections));
    output
}

fn serialize_for_arith(command: &ForArithCommand) -> String {
    let mut output = format!(
        "for (( {}; {}; {} )); do ",
        command.init, command.condition, command.update
    );
    output.push_str(&serialize_inline_statements(&command.body));
    output.push_str("; done");
    output.push_str(&serialize_redirections(&command.redirections));
    output
}

fn serialize_loop(keyword: &str, command: &LoopCommand) -> String {
    let mut output = format!(
        "{} {}; do {}; done",
        keyword,
        serialize_inline_statements(&command.condition),
        serialize_inline_statements(&command.body)
    );
    output.push_str(&serialize_redirections(&command.redirections));
    output
}

fn serialize_case(command: &CaseCommand) -> String {
    let mut output = format!("case {} in", serialize_word(&command.word));
    for item in &command.items {
        output.push(' ');
        output.push_str(
            &item
                .patterns
                .iter()
                .map(serialize_word)
                .collect::<Vec<_>>()
                .join("|"),
        );
        output.push_str(") ");
        output.push_str(&serialize_inline_statements(&item.body));
        output.push_str(match item.terminator {
            CaseTerminator::Break => " ;;",
            CaseTerminator::FallThrough => " ;&",
            CaseTerminator::ContinueMatching => " ;;&",
        });
    }
    output.push_str(" esac");
    output.push_str(&serialize_redirections(&command.redirections));
    output
}

fn serialize_redirections(redirections: &[Redirection]) -> String {
    let mut here_docs = Vec::new();
    redirections
        .iter()
        .map(|redirection| format!(" {}", serialize_redirection(redirection, &mut here_docs)))
        .collect::<String>()
}

fn serialize_redirection(redirection: &Redirection, here_docs: &mut Vec<String>) -> String {
    let mut output = redirection.fd.map(|fd| fd.to_string()).unwrap_or_default();
    output.push_str(match redirection.operator {
        RedirectionOperator::Input => "<",
        RedirectionOperator::Output => ">",
        RedirectionOperator::Append => ">>",
        RedirectionOperator::DuplicateInput => "<&",
        RedirectionOperator::DuplicateOutput => ">&",
        RedirectionOperator::ReadWrite => "<>",
        RedirectionOperator::Clobber => ">|",
        RedirectionOperator::OutputBoth => "&>",
        RedirectionOperator::AppendBoth => "&>>",
        RedirectionOperator::HereString => "<<<",
        RedirectionOperator::HereDoc => "<<",
        RedirectionOperator::HereDocStripTabs => "<<-",
    });
    if let Some(here_doc) = &redirection.here_doc {
        let delimiter = if here_doc.quoted {
            format!("'{}'", here_doc.delimiter.replace('\'', "'\\''"))
        } else {
            here_doc.delimiter.clone()
        };
        output.push_str(&delimiter);
        here_docs.push(serialize_here_doc_body(here_doc));
    } else {
        output.push(' ');
        output.push_str(&serialize_word(&redirection.target));
    }
    output
}

fn serialize_here_doc_body(here_doc: &HereDoc) -> String {
    let mut output = here_doc.content.clone();
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(&here_doc.delimiter);
    output
}

fn serialize_simple_command_without_redirections(command: &SimpleCommand) -> String {
    let mut parts = Vec::new();
    if let Some(name) = &command.name {
        parts.push(serialize_word(name));
    }
    parts.extend(command.args.iter().map(serialize_word));
    parts.join(" ")
}

fn simple_command_for_tee(command: &Command) -> Option<&SimpleCommand> {
    match command {
        Command::Simple(command) if command.name.is_some() => Some(command),
        _ => None,
    }
}

fn make_pipestatus_save(original_indices: &[usize]) -> Pipeline {
    Pipeline {
        commands: vec![Command::Simple(SimpleCommand {
            assignments: original_indices
                .iter()
                .enumerate()
                .map(|(index, original_index)| Assignment {
                    name: format!("__tps{index}"),
                    value: Some(Word {
                        parts: vec![WordPart::Parameter(ParameterExpansion {
                            parameter: format!("PIPESTATUS[{original_index}]"),
                            operation: None,
                        })],
                    }),
                    append: false,
                    array: None,
                    index: None,
                })
                .collect(),
            name: None,
            args: Vec::new(),
            redirections: Vec::new(),
        })],
        negated: false,
        pipe_stderr: Vec::new(),
    }
}

fn make_pipestatus_restore(count: usize, negated: bool) -> Pipeline {
    Pipeline {
        commands: (0..count)
            .map(|index| {
                Command::Subshell(vec![Statement {
                    pipelines: vec![Pipeline {
                        commands: vec![Command::Simple(SimpleCommand {
                            assignments: Vec::new(),
                            name: Some(Word::literal("exit")),
                            args: vec![Word {
                                parts: vec![WordPart::Parameter(ParameterExpansion {
                                    parameter: format!("__tps{index}"),
                                    operation: None,
                                })],
                            }],
                            redirections: Vec::new(),
                        })],
                        negated: false,
                        pipe_stderr: Vec::new(),
                    }],
                    operators: Vec::new(),
                    background: false,
                }])
            })
            .collect(),
        negated,
        pipe_stderr: (1..count).map(|_| false).collect(),
    }
}

fn serialize_word(word: &Word) -> String {
    word.parts
        .iter()
        .map(serialize_word_part)
        .collect::<String>()
}

fn serialize_word_part(part: &WordPart) -> String {
    match part {
        WordPart::Literal(value) => value.clone(),
        WordPart::SingleQuoted(value) => format!("'{}'", value.replace('\'', "'\\''")),
        WordPart::DoubleQuoted(parts) => {
            let content = parts
                .iter()
                .map(serialize_double_quoted_part)
                .collect::<String>();
            format!("\"{content}\"")
        }
        WordPart::Escaped(value) => format!("\\{value}"),
        WordPart::Parameter(parameter) => serialize_parameter(parameter),
        WordPart::CommandSubstitution { body, legacy } => {
            if *legacy {
                format!("`{}`", serialize(body))
            } else {
                format!("$({})", serialize(body))
            }
        }
        WordPart::Arithmetic(expression) => format!("$(({}))", expression.source),
        WordPart::Brace(brace) => serialize_brace(brace),
        WordPart::Tilde { user } => format!("~{}", user.as_deref().unwrap_or_default()),
    }
}

fn serialize_double_quoted_part(part: &WordPart) -> String {
    match part {
        WordPart::Literal(value) | WordPart::Escaped(value) => value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('$', "\\$")
            .replace('`', "\\`"),
        WordPart::SingleQuoted(value) => value.clone(),
        _ => serialize_word_part(part),
    }
}

fn serialize_parameter(parameter: &ParameterExpansion) -> String {
    match &parameter.operation {
        Some(ParameterOperation::Length) => format!("${{#{}}}", parameter.parameter),
        Some(ParameterOperation::DefaultValue { word, check_empty }) => format!(
            "${{{}{}{}}}",
            parameter.parameter,
            if *check_empty { ":-" } else { "-" },
            serialize_word(word)
        ),
        Some(ParameterOperation::AssignDefault { word, check_empty }) => format!(
            "${{{}{}{}}}",
            parameter.parameter,
            if *check_empty { ":=" } else { "=" },
            serialize_word(word)
        ),
        Some(ParameterOperation::UseAlternative { word, check_empty }) => format!(
            "${{{}{}{}}}",
            parameter.parameter,
            if *check_empty { ":+" } else { "+" },
            serialize_word(word)
        ),
        None if is_valid_name(&parameter.parameter)
            || is_unbraced_special_parameter_name(&parameter.parameter) =>
        {
            format!("${}", parameter.parameter)
        }
        None => format!("${{{}}}", parameter.parameter),
    }
}

fn serialize_brace(brace: &BraceExpansion) -> String {
    if let [BraceItem::Range { start, end, step }] = brace.items.as_slice() {
        return if let Some(step) = step {
            format!("{{{start}..{end}..{step}}}")
        } else {
            format!("{{{start}..{end}}}")
        };
    }
    let items = brace
        .items
        .iter()
        .map(|item| match item {
            BraceItem::Word(word) => serialize_word(word),
            BraceItem::Range { start, end, step } => {
                if let Some(step) = step {
                    format!("{start}..{end}..{step}")
                } else {
                    format!("{start}..{end}")
                }
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("{{{items}}}")
}

fn is_unbraced_special_parameter_name(name: &str) -> bool {
    matches!(name, "?" | "#" | "@" | "*" | "$" | "!" | "-")
        || name
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
            && name.chars().count() == 1
}

fn collect_statements(statements: &[Statement], names: &mut std::collections::BTreeSet<String>) {
    for statement in statements {
        for pipeline in &statement.pipelines {
            for command in &pipeline.commands {
                collect_command(command, names);
            }
        }
    }
}

fn collect_command(command: &Command, names: &mut std::collections::BTreeSet<String>) {
    match command {
        Command::Simple(command) => {
            if let Some(name) = command.name.as_ref().and_then(extract_literal_command_name) {
                names.insert(name.to_string());
            }
            if let Some(name) = &command.name {
                collect_word(name, names);
            }
            for arg in &command.args {
                collect_word(arg, names);
            }
            for assignment in &command.assignments {
                if let Some(value) = &assignment.value {
                    collect_word(value, names);
                }
                if let Some(array) = &assignment.array {
                    for word in array {
                        collect_word(word, names);
                    }
                }
            }
        }
        Command::If(command) => {
            for clause in &command.clauses {
                collect_statements(&clause.condition, names);
                collect_statements(&clause.body, names);
            }
            collect_statements(&command.else_body, names);
        }
        Command::For(command) => {
            for word in &command.words {
                collect_word(word, names);
            }
            collect_statements(&command.body, names);
        }
        Command::ForArith(command) => {
            collect_statements(&command.body, names);
        }
        Command::While(command) | Command::Until(command) => {
            collect_statements(&command.condition, names);
            collect_statements(&command.body, names);
        }
        Command::Case(command) => {
            collect_word(&command.word, names);
            for item in &command.items {
                collect_statements(&item.body, names);
            }
        }
        Command::FunctionDef(function) => collect_command(&function.body, names),
        Command::Subshell(body) | Command::Group(body) => collect_statements(body, names),
        Command::Arithmetic(_) | Command::Conditional(_) => {}
    }
}

fn collect_word(word: &Word, names: &mut std::collections::BTreeSet<String>) {
    for part in &word.parts {
        collect_word_part(part, names);
    }
}

fn collect_word_part(part: &WordPart, names: &mut std::collections::BTreeSet<String>) {
    match part {
        WordPart::DoubleQuoted(parts) => {
            for part in parts {
                collect_word_part(part, names);
            }
        }
        WordPart::CommandSubstitution { body, .. } => collect_statements(&body.statements, names),
        WordPart::Parameter(parameter) => match &parameter.operation {
            Some(ParameterOperation::DefaultValue { word, .. })
            | Some(ParameterOperation::AssignDefault { word, .. })
            | Some(ParameterOperation::UseAlternative { word, .. }) => collect_word(word, names),
            Some(ParameterOperation::Length) | None => {}
        },
        WordPart::Brace(brace) => {
            for item in &brace.items {
                if let BraceItem::Word(word) = item {
                    collect_word(word, names);
                }
            }
        }
        WordPart::Literal(_)
        | WordPart::SingleQuoted(_)
        | WordPart::Escaped(_)
        | WordPart::Arithmetic(_)
        | WordPart::Tilde { .. } => {}
    }
}

fn extract_literal_command_name(word: &Word) -> Option<&str> {
    match word.parts.as_slice() {
        [WordPart::Literal(value)] => Some(value.as_str()),
        _ => None,
    }
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(input: &str) -> ShellResult<Self> {
        Ok(Self {
            tokens: Lexer::new(input).tokenize()?,
            pos: 0,
        })
    }

    fn parse(mut self) -> ShellResult<Script> {
        let statements = self.parse_statements_until(&[], false, false, false)?;
        self.expect_eof()?;
        Ok(Script { statements })
    }

    fn current(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn peek(&self, offset: usize) -> &TokenKind {
        self.tokens
            .get(self.pos + offset)
            .map(|token| &token.kind)
            .unwrap_or(&TokenKind::Eof)
    }

    fn advance(&mut self) -> Token {
        let token = self.current().clone();
        if !matches!(token.kind, TokenKind::Eof) {
            self.pos += 1;
        }
        token
    }

    fn parse_statements_until(
        &mut self,
        stop_words: &[&str],
        stop_right_paren: bool,
        stop_right_brace: bool,
        stop_case_end: bool,
    ) -> ShellResult<Vec<Statement>> {
        let mut statements = Vec::new();
        loop {
            self.skip_newlines();
            if self.at_stop(
                stop_words,
                stop_right_paren,
                stop_right_brace,
                stop_case_end,
            ) {
                break;
            }
            if matches!(
                self.current().kind,
                TokenKind::Eof | TokenKind::Semicolon | TokenKind::DoubleSemicolon
            ) {
                break;
            }

            statements.push(self.parse_statement()?);

            match self.current().kind {
                TokenKind::Semicolon | TokenKind::Newline => {
                    self.advance();
                    self.skip_newlines();
                    if matches!(
                        self.current().kind,
                        TokenKind::Semicolon | TokenKind::DoubleSemicolon
                    ) && !self.at_stop(
                        stop_words,
                        stop_right_paren,
                        stop_right_brace,
                        stop_case_end,
                    ) {
                        return Err(self.error_here("unexpected separator"));
                    }
                }
                _ => {}
            }
        }
        Ok(statements)
    }

    fn at_stop(
        &self,
        stop_words: &[&str],
        stop_right_paren: bool,
        stop_right_brace: bool,
        stop_case_end: bool,
    ) -> bool {
        match &self.current().kind {
            TokenKind::Eof => true,
            TokenKind::RightParen => stop_right_paren,
            TokenKind::RightBrace => stop_right_brace,
            TokenKind::DoubleSemicolon
            | TokenKind::SemicolonAmp
            | TokenKind::DoubleSemicolonAmp => stop_case_end,
            TokenKind::Word(word) => word
                .plain_text()
                .is_some_and(|text| stop_words.contains(&text.as_str())),
            _ => false,
        }
    }

    fn skip_newlines(&mut self) {
        while matches!(self.current().kind, TokenKind::Newline) {
            self.advance();
        }
    }

    fn skip_separators(&mut self) {
        while matches!(
            self.current().kind,
            TokenKind::Newline | TokenKind::Semicolon
        ) {
            self.advance();
        }
    }

    fn parse_statement(&mut self) -> ShellResult<Statement> {
        let mut pipelines = vec![self.parse_pipeline()?];
        let mut operators = Vec::new();
        loop {
            let operator = match self.current().kind {
                TokenKind::AndIf => ListOperator::And,
                TokenKind::OrIf => ListOperator::Or,
                _ => break,
            };
            self.advance();
            operators.push(operator);
            pipelines.push(self.parse_pipeline()?);
        }

        let background = if matches!(self.current().kind, TokenKind::Amp) {
            self.advance();
            true
        } else {
            false
        };

        Ok(Statement {
            pipelines,
            operators,
            background,
        })
    }

    fn parse_pipeline(&mut self) -> ShellResult<Pipeline> {
        let mut bang_count = 0usize;
        while matches!(self.current().kind, TokenKind::Bang) {
            bang_count += 1;
            self.advance();
        }

        let mut commands = vec![self.parse_command()?];
        let mut pipe_stderr = Vec::new();
        loop {
            match self.current().kind {
                TokenKind::Pipe => {
                    self.advance();
                    pipe_stderr.push(false);
                    commands.push(self.parse_command()?);
                }
                TokenKind::PipeStderr => {
                    self.advance();
                    pipe_stderr.push(true);
                    commands.push(self.parse_command()?);
                }
                _ => break,
            }
        }

        Ok(Pipeline {
            commands,
            negated: bang_count % 2 == 1,
            pipe_stderr,
        })
    }

    fn parse_command(&mut self) -> ShellResult<Command> {
        match self.current().kind.clone() {
            TokenKind::ArithmeticCommand(expression) => {
                self.advance();
                return Ok(Command::Arithmetic(expression));
            }
            TokenKind::ConditionalCommand(expression) => {
                self.advance();
                return Ok(Command::Conditional(expression));
            }
            TokenKind::LeftParen => return self.parse_subshell(),
            TokenKind::LeftBrace => return self.parse_group(),
            _ => {}
        }

        let Some(word_text) = self.current_word_text() else {
            if matches!(self.current().kind, TokenKind::Word(_)) {
                return self.parse_simple_command().map(Command::Simple);
            }
            if matches!(self.current().kind, TokenKind::Redirection(_)) {
                return self.parse_simple_command().map(Command::Simple);
            }
            return Err(self.error_here("expected command"));
        };

        match word_text.as_str() {
            "if" => self.parse_if(),
            "for" => self.parse_for(),
            "while" => self.parse_loop(true),
            "until" => self.parse_loop(false),
            "case" => self.parse_case(),
            "function" => self.parse_function_keyword(),
            _ if self.is_function_def_start() => self.parse_function_name_syntax(),
            _ => self.parse_simple_command().map(Command::Simple),
        }
    }

    fn parse_simple_command(&mut self) -> ShellResult<SimpleCommand> {
        let mut command = SimpleCommand::default();

        loop {
            let reserved_words_terminate = command.name.is_none()
                && command.assignments.is_empty()
                && command.redirections.is_empty();
            if self.is_command_terminator(reserved_words_terminate) {
                break;
            }

            if let TokenKind::Word(fd_word) = self.current().kind.clone()
                && let Some(fd_text) = fd_word.plain_text()
                && fd_text.chars().all(|character| character.is_ascii_digit())
                && matches!(self.peek(1), TokenKind::Redirection(_))
            {
                let fd = fd_text.parse::<u8>().ok();
                self.advance();
                let operator = match self.advance().kind {
                    TokenKind::Redirection(operator) => operator,
                    _ => unreachable!("peeked redirection"),
                };
                command
                    .redirections
                    .push(self.parse_redirection(fd, operator)?);
                continue;
            }

            if let TokenKind::Redirection(operator) = self.current().kind.clone() {
                self.advance();
                command
                    .redirections
                    .push(self.parse_redirection(None, operator)?);
                continue;
            }

            let TokenKind::Word(word) = self.current().kind.clone() else {
                break;
            };

            if command.name.is_none()
                && let Some((name, index, append, value)) = split_assignment_word(&word)
            {
                self.advance();
                let array = if matches!(self.current().kind, TokenKind::LeftParen) {
                    Some(self.parse_array_assignment()?)
                } else {
                    None
                };
                command.assignments.push(Assignment {
                    name,
                    value,
                    append,
                    array,
                    index,
                });
                continue;
            }

            self.advance();
            if command.name.is_none() {
                command.name = Some(word);
            } else {
                command.args.push(word);
            }
        }

        if command.name.is_none()
            && command.assignments.is_empty()
            && command.redirections.is_empty()
        {
            Err(self.error_here("empty command"))
        } else {
            Ok(command)
        }
    }

    fn parse_redirection(
        &mut self,
        fd: Option<u8>,
        operator: RedirectionOperator,
    ) -> ShellResult<Redirection> {
        match self.advance().kind {
            TokenKind::Word(target) => Ok(Redirection {
                fd,
                operator,
                target,
                here_doc: None,
            }),
            TokenKind::HereDocTarget { target, here_doc } => Ok(Redirection {
                fd,
                operator,
                target,
                here_doc: Some(here_doc),
            }),
            _ => Err(self.error_previous("expected redirection target")),
        }
    }

    fn parse_array_assignment(&mut self) -> ShellResult<Vec<Word>> {
        self.expect_token(TokenKindName::LeftParen)?;
        let mut words = Vec::new();
        while !matches!(self.current().kind, TokenKind::RightParen | TokenKind::Eof) {
            match self.advance().kind {
                TokenKind::Word(word) => words.push(word),
                TokenKind::Newline | TokenKind::Semicolon => {}
                _ => return Err(self.error_previous("expected array item")),
            }
        }
        self.expect_token(TokenKindName::RightParen)?;
        Ok(words)
    }

    fn parse_subshell(&mut self) -> ShellResult<Command> {
        self.expect_token(TokenKindName::LeftParen)?;
        let body = self.parse_statements_until(&[], true, false, false)?;
        self.expect_token(TokenKindName::RightParen)?;
        Ok(Command::Subshell(body))
    }

    fn parse_group(&mut self) -> ShellResult<Command> {
        self.expect_token(TokenKindName::LeftBrace)?;
        let body = self.parse_statements_until(&[], false, true, false)?;
        self.expect_token(TokenKindName::RightBrace)?;
        Ok(Command::Group(body))
    }

    fn parse_if(&mut self) -> ShellResult<Command> {
        self.expect_word("if")?;
        let mut clauses = Vec::new();
        let condition = self.parse_statements_until(&["then"], false, false, false)?;
        self.expect_word("then")?;
        let body = self.parse_statements_until(&["elif", "else", "fi"], false, false, false)?;
        clauses.push(IfClause { condition, body });

        while self.current_word_is("elif") {
            self.advance();
            let condition = self.parse_statements_until(&["then"], false, false, false)?;
            self.expect_word("then")?;
            let body = self.parse_statements_until(&["elif", "else", "fi"], false, false, false)?;
            clauses.push(IfClause { condition, body });
        }

        let else_body = if self.current_word_is("else") {
            self.advance();
            self.parse_statements_until(&["fi"], false, false, false)?
        } else {
            Vec::new()
        };
        self.expect_word("fi")?;
        Ok(Command::If(IfCommand {
            clauses,
            else_body,
            redirections: Vec::new(),
        }))
    }

    fn parse_for(&mut self) -> ShellResult<Command> {
        self.expect_word("for")?;

        // C-style `for (( init; cond; update )); do ... done`. The tokenizer
        // collapses the `(( ... ))` header into a single arithmetic-command
        // token whose source is the three semicolon-separated clauses.
        if let TokenKind::ArithmeticCommand(expression) = &self.current().kind {
            let source = expression.source.clone();
            self.advance();
            let (init, condition, update) = split_for_arith_clauses(&source);
            self.skip_separators();
            self.expect_word("do")?;
            let body = self.parse_statements_until(&["done"], false, false, false)?;
            self.expect_word("done")?;
            return Ok(Command::ForArith(ForArithCommand {
                init,
                condition,
                update,
                body,
                redirections: Vec::new(),
            }));
        }

        let variable = self
            .take_word_text()
            .ok_or_else(|| self.error_here("expected for variable"))?;
        // An invalid identifier (e.g. `for 123 in ...`) parses fine but is
        // rejected at execution time with a runtime error, mirroring bash.

        self.skip_separators();
        let mut words = Vec::new();
        let mut has_in_clause = false;
        if self.current_word_is("in") {
            has_in_clause = true;
            self.advance();
            loop {
                self.skip_separators();
                if self.current_word_is("do") {
                    break;
                }
                match self.current().kind.clone() {
                    TokenKind::Word(word) => {
                        self.advance();
                        words.push(word);
                    }
                    _ => return Err(self.error_here("expected do in for loop")),
                }
            }
        } else if !self.current_word_is("do") {
            return Err(self.error_here("expected in or do in for loop"));
        }

        self.expect_word("do")?;
        let body = self.parse_statements_until(&["done"], false, false, false)?;
        self.expect_word("done")?;
        Ok(Command::For(ForCommand {
            variable,
            words,
            has_in_clause,
            body,
            redirections: Vec::new(),
        }))
    }

    fn parse_loop(&mut self, is_while: bool) -> ShellResult<Command> {
        if is_while {
            self.expect_word("while")?;
        } else {
            self.expect_word("until")?;
        }
        let condition = self.parse_statements_until(&["do"], false, false, false)?;
        self.expect_word("do")?;
        let body = self.parse_statements_until(&["done"], false, false, false)?;
        self.expect_word("done")?;
        let command = LoopCommand {
            condition,
            body,
            redirections: Vec::new(),
        };
        Ok(if is_while {
            Command::While(command)
        } else {
            Command::Until(command)
        })
    }

    fn parse_case(&mut self) -> ShellResult<Command> {
        self.expect_word("case")?;
        let word = match self.advance().kind {
            TokenKind::Word(word) => word,
            _ => return Err(self.error_previous("expected case word")),
        };
        self.expect_word("in")?;
        self.skip_separators();
        let mut items = Vec::new();
        while !self.current_word_is("esac") && !matches!(self.current().kind, TokenKind::Eof) {
            let mut patterns = Vec::new();
            if matches!(self.current().kind, TokenKind::LeftParen) {
                self.advance();
            }
            loop {
                match self.advance().kind {
                    TokenKind::Word(word) => patterns.push(word),
                    _ => return Err(self.error_previous("expected case pattern")),
                }
                if matches!(self.current().kind, TokenKind::Pipe) {
                    self.advance();
                    continue;
                }
                break;
            }
            self.expect_token(TokenKindName::RightParen)?;
            let body = self.parse_statements_until(&["esac"], false, false, true)?;
            let terminator = match self.current().kind {
                TokenKind::DoubleSemicolonAmp => {
                    self.advance();
                    CaseTerminator::ContinueMatching
                }
                TokenKind::SemicolonAmp => {
                    self.advance();
                    CaseTerminator::FallThrough
                }
                TokenKind::DoubleSemicolon => {
                    self.advance();
                    CaseTerminator::Break
                }
                _ => CaseTerminator::Break,
            };
            items.push(CaseItem {
                patterns,
                body,
                terminator,
            });
            self.skip_separators();
        }
        self.expect_word("esac")?;
        Ok(Command::Case(CaseCommand {
            word,
            items,
            redirections: Vec::new(),
        }))
    }

    fn parse_function_keyword(&mut self) -> ShellResult<Command> {
        self.expect_word("function")?;
        let name = self
            .take_word_text()
            .ok_or_else(|| self.error_here("expected function name"))?;
        if matches!(self.current().kind, TokenKind::LeftParen)
            && matches!(self.peek(1), TokenKind::RightParen)
        {
            self.advance();
            self.advance();
        }
        let body = Box::new(self.parse_command()?);
        Ok(Command::FunctionDef(FunctionDef {
            name,
            body,
            redirections: Vec::new(),
        }))
    }

    fn parse_function_name_syntax(&mut self) -> ShellResult<Command> {
        let name = self
            .take_word_text()
            .ok_or_else(|| self.error_here("expected function name"))?;
        self.expect_token(TokenKindName::LeftParen)?;
        self.expect_token(TokenKindName::RightParen)?;
        let body = Box::new(self.parse_command()?);
        Ok(Command::FunctionDef(FunctionDef {
            name,
            body,
            redirections: Vec::new(),
        }))
    }

    fn is_function_def_start(&self) -> bool {
        matches!(self.current().kind, TokenKind::Word(_))
            && matches!(self.peek(1), TokenKind::LeftParen)
            && matches!(self.peek(2), TokenKind::RightParen)
    }

    fn is_command_terminator(&self, reserved_words_terminate: bool) -> bool {
        matches!(
            self.current().kind,
            TokenKind::Eof
                | TokenKind::Semicolon
                | TokenKind::Newline
                | TokenKind::AndIf
                | TokenKind::OrIf
                | TokenKind::Pipe
                | TokenKind::PipeStderr
                | TokenKind::RightParen
                | TokenKind::RightBrace
                | TokenKind::DoubleSemicolon
        ) || (reserved_words_terminate
            && self.current_word_is_any(&["then", "else", "elif", "fi", "do", "done", "esac"]))
    }

    fn current_word_text(&self) -> Option<String> {
        match &self.current().kind {
            TokenKind::Word(word) => word.plain_text(),
            _ => None,
        }
    }

    fn take_word_text(&mut self) -> Option<String> {
        let text = self.current_word_text()?;
        self.advance();
        Some(text)
    }

    fn current_word_is(&self, expected: &str) -> bool {
        self.current_word_text().as_deref() == Some(expected)
    }

    fn current_word_is_any(&self, expected: &[&str]) -> bool {
        self.current_word_text()
            .is_some_and(|text| expected.contains(&text.as_str()))
    }

    fn expect_word(&mut self, expected: &str) -> ShellResult<()> {
        if self.current_word_is(expected) {
            self.advance();
            Ok(())
        } else {
            Err(self.error_here(format!("expected {expected}")))
        }
    }

    fn expect_token(&mut self, expected: TokenKindName) -> ShellResult<()> {
        if expected.matches(&self.current().kind) {
            self.advance();
            Ok(())
        } else {
            Err(self.error_here(format!("expected {}", expected.label())))
        }
    }

    fn expect_eof(&self) -> ShellResult<()> {
        if matches!(self.current().kind, TokenKind::Eof) {
            Ok(())
        } else {
            Err(self.error_here("unexpected token"))
        }
    }

    fn error_here(&self, message: impl Into<String>) -> ShellError {
        ShellError::new(message, self.current().line, self.current().column)
    }

    fn error_previous(&self, message: impl Into<String>) -> ShellError {
        let index = self.pos.saturating_sub(1);
        let token = &self.tokens[index];
        ShellError::new(message, token.line, token.column)
    }
}

#[derive(Debug, Clone, Copy)]
enum TokenKindName {
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
}

impl TokenKindName {
    fn matches(self, kind: &TokenKind) -> bool {
        matches!(
            (self, kind),
            (Self::LeftParen, TokenKind::LeftParen)
                | (Self::RightParen, TokenKind::RightParen)
                | (Self::LeftBrace, TokenKind::LeftBrace)
                | (Self::RightBrace, TokenKind::RightBrace)
        )
    }

    fn label(self) -> &'static str {
        match self {
            Self::LeftParen => "(",
            Self::RightParen => ")",
            Self::LeftBrace => "{",
            Self::RightBrace => "}",
        }
    }
}

fn split_assignment_word(word: &Word) -> Option<(String, Option<usize>, bool, Option<Word>)> {
    let WordPart::Literal(first) = word.parts.first()? else {
        return None;
    };
    let equals = first.find('=')?;
    let mut lhs = first[..equals].to_string();
    let append = lhs.ends_with('+');
    if append {
        lhs.pop();
    }
    let (name, index) = parse_assignment_target(&lhs)?;

    let mut value_parts = Vec::new();
    if equals + 1 < first.len() {
        value_parts.push(WordPart::Literal(first[equals + 1..].to_string()));
    }
    value_parts.extend(word.parts.iter().skip(1).cloned());

    Some((
        name,
        index,
        append,
        Some(Word { parts: value_parts }).filter(|value| !value.parts.is_empty()),
    ))
}

fn parse_assignment_target(lhs: &str) -> Option<(String, Option<usize>)> {
    if is_valid_name(lhs) {
        return Some((lhs.to_string(), None));
    }
    let (name, index) = parse_array_reference(lhs)?;
    let ArrayIndex::Index(index) = index else {
        return None;
    };
    Some((name.to_string(), Some(index)))
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExecOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl ExecOutput {
    fn append(&mut self, other: ExecOutput) {
        self.stdout.push_str(&other.stdout);
        self.stderr.push_str(&other.stderr);
        self.exit_code = other.exit_code;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandInvocation {
    pub name: String,
    pub args: Vec<String>,
    pub stdin: String,
    pub env: BTreeMap<String, String>,
    pub redirections: Vec<AppliedRedirection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl CommandResult {
    pub fn success(stdout: impl Into<String>) -> Self {
        Self {
            stdout: stdout.into(),
            stderr: String::new(),
            exit_code: 0,
        }
    }

    pub fn failure(stderr: impl Into<String>, exit_code: i32) -> Self {
        Self {
            stdout: String::new(),
            stderr: stderr.into(),
            exit_code,
        }
    }
}

pub trait CommandDispatcher {
    fn dispatch(
        &mut self,
        invocation: CommandInvocation,
        files: &mut ShellVirtualFileSystem,
    ) -> CommandResult;
}

#[derive(Debug, Clone, Default)]
pub struct MissingCommandDispatcher;

impl CommandDispatcher for MissingCommandDispatcher {
    fn dispatch(
        &mut self,
        invocation: CommandInvocation,
        _files: &mut ShellVirtualFileSystem,
    ) -> CommandResult {
        CommandResult::failure(format!("{}: command not found\n", invocation.name), 127)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShellVirtualFileSystem {
    files: BTreeMap<String, String>,
}

impl ShellVirtualFileSystem {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_files(
        files: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        Self {
            files: files
                .into_iter()
                .map(|(path, content)| (path.into(), content.into()))
                .collect(),
        }
    }

    pub fn read_to_string(&self, path: &str) -> Option<&str> {
        self.files.get(path).map(String::as_str)
    }

    pub fn write(&mut self, path: impl Into<String>, content: impl Into<String>) {
        self.files.insert(path.into(), content.into());
    }

    pub fn append(&mut self, path: impl Into<String>, content: &str) {
        self.files.entry(path.into()).or_default().push_str(content);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedRedirection {
    pub fd: Option<u8>,
    pub operator: RedirectionOperator,
    pub target: String,
    pub here_doc: Option<HereDoc>,
}

/// Exit code reported when an execution safety limit (recursion depth, command
/// count, loop iterations) is exceeded. Mirrors upstream
/// `ExecutionLimitError.EXIT_CODE`.
pub const EXECUTION_LIMIT_EXIT_CODE: i32 = 126;

/// Usage text emitted by the `set` builtin for `set --help` and appended to
/// invalid-option errors. Mirrors upstream
/// `packages/just-bash/src/interpreter/builtins/set.ts` `SET_USAGE`.
const SET_USAGE: &str = "set: usage: set [-eux] [+eux] [-o option] [+o option]\nOptions:\n  -e            Exit immediately if a command exits with non-zero status\n  +e            Disable -e\n  -u            Treat unset variables as an error when substituting\n  +u            Disable -u\n  -x            Print commands and their arguments as they are executed\n  +x            Disable -x\n  -o errexit    Same as -e\n  +o errexit    Disable errexit\n  -o nounset    Same as -u\n  +o nounset    Disable nounset\n  -o pipefail   Return status of last failing command in pipeline\n  +o pipefail   Disable pipefail\n  -o xtrace     Same as -x\n  +o xtrace     Disable xtrace\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionLimits {
    pub max_loop_iterations: usize,
    /// Maximum total commands executed during a single `exec` call before the
    /// interpreter aborts with an execution-limit error. Mirrors upstream
    /// `maxCommandCount`.
    pub max_commands: usize,
    /// Maximum function call (recursion) depth before the interpreter aborts
    /// with a "maximum recursion depth" error. Mirrors upstream `maxCallDepth`.
    pub max_call_depth: usize,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_loop_iterations: 10_000,
            max_commands: 10_000,
            max_call_depth: 100,
        }
    }
}

/// Insertion-ordered alias storage.
///
/// Bash lists aliases in the order they were first defined and re-defining an
/// existing alias updates its value in place without changing its position.
/// Upstream just-bash keeps aliases inside the insertion-ordered environment
/// `Map`; this mirrors that ordering for `alias` listing parity.
#[derive(Debug, Clone, Default)]
struct AliasTable {
    entries: Vec<(String, String)>,
}

impl AliasTable {
    fn insert(&mut self, name: String, value: String) {
        if let Some(entry) = self.entries.iter_mut().find(|(key, _)| key == &name) {
            entry.1 = value;
        } else {
            self.entries.push((name, value));
        }
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn remove(&mut self, name: &str) -> bool {
        if let Some(index) = self.entries.iter().position(|(key, _)| key == name) {
            self.entries.remove(index);
            true
        } else {
            false
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
    }

    fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }
}

#[derive(Debug, Clone, Default)]
pub struct ShellState {
    env: BTreeMap<String, String>,
    arrays: BTreeMap<String, Vec<String>>,
    aliases: AliasTable,
    functions: BTreeMap<String, FunctionDef>,
    positionals: Vec<String>,
    local_scopes: Vec<BTreeMap<String, Option<String>>>,
    last_status: i32,
    pipefail: bool,
    /// `set -e` / `set -o errexit`: abort the script on the first command that
    /// returns a non-zero status outside of a tested context.
    errexit: bool,
    /// Depth counter; while > 0 the current statements run in a tested context
    /// (an `if`/`while`/`until` condition or the left side of `&&`/`||`) where
    /// errexit must not fire.
    errexit_suppressed: u32,
    xtrace: bool,
    exited: Option<i32>,
    /// Pending execution-limit abort (recursion depth, command count, or loop
    /// iterations exceeded). Carries the diagnostic message. Once set it
    /// short-circuits all further statement execution like an `exit`, and the
    /// top-level `exec` reports it with exit code [`EXECUTION_LIMIT_EXIT_CODE`].
    execution_limit: Option<String>,
    /// Pending `return` signal raised by the `return` builtin. Carries the
    /// status the enclosing function call should resolve to. Cleared once the
    /// nearest `call_function` consumes it. Only valid while `function_depth`
    /// is greater than 0.
    returning: Option<i32>,
    /// Number of shell functions currently executing on the call stack. The
    /// `return` builtin is only valid (and only sets `returning`) when this is
    /// greater than 0; otherwise it is an error.
    function_depth: u32,
    /// Pending loop-control signal raised by the `break`/`continue` builtins.
    /// Carries the number of loop levels still to unwind (bash `break n` /
    /// `continue n`). Cleared once the target loop consumes it.
    loop_control: Option<LoopControl>,
    command_count: usize,
    alias_depth: usize,
    /// Number of loops the interpreter is currently executing inside (in this
    /// shell/subshell context). `break`/`continue` are no-ops at depth 0.
    loop_depth: u32,
    /// Number of subshells currently open. When `break`/`continue` runs with no
    /// enclosing loop *inside* a subshell, bash terminates that subshell (the
    /// builtin behaves like a local `exit`) without disturbing the parent loop.
    subshell_depth: u32,
    /// Pending arithmetic-evaluation error (division/modulo by zero, negative
    /// exponent). Set by the arithmetic evaluator and drained by the enclosing
    /// command so the error is reported on stderr with a non-zero exit code.
    arith_error: Option<String>,
}

/// Active `break`/`continue` signal propagating up through nested compound
/// commands until the requested number of enclosing loops have been unwound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopControl {
    Break(u32),
    Continue(u32),
}

/// Result of resolving a pending loop-control signal against the loop that is
/// currently unwinding.
enum LoopFlow {
    /// Proceed to the next iteration of this loop.
    Continue,
    /// Stop iterating this loop.
    BreakLoop,
}

impl ShellState {
    pub fn set_var(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.assign_var(name.into(), value.into());
    }

    pub fn get_var(&self, name: &str) -> Option<&str> {
        self.lookup_var(name)
    }

    pub fn set_alias(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.aliases.insert(name.into(), value.into());
    }

    pub fn get_alias(&self, name: &str) -> Option<&str> {
        self.aliases.get(name)
    }

    pub fn set_array(&mut self, name: impl Into<String>, values: Vec<String>) {
        self.arrays.insert(name.into(), values);
    }

    fn lookup_var(&self, name: &str) -> Option<&str> {
        for scope in self.local_scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                return value.as_deref();
            }
        }
        self.env.get(name).map(String::as_str)
    }

    fn assign_var(&mut self, name: String, value: String) {
        for scope in self.local_scopes.iter_mut().rev() {
            if let Some(slot) = scope.get_mut(&name) {
                *slot = Some(value);
                return;
            }
        }
        self.env.insert(name, value);
    }

    fn unset_var(&mut self, name: &str) {
        for scope in self.local_scopes.iter_mut().rev() {
            if scope.remove(name).is_some() {
                return;
            }
        }
        self.env.remove(name);
        self.arrays.remove(name);
    }

    fn declare_local(&mut self, name: String, value: Option<String>) -> bool {
        let Some(scope) = self.local_scopes.last_mut() else {
            return false;
        };
        scope.insert(name, value);
        true
    }
}

pub struct Interpreter<D> {
    dispatcher: D,
    state: ShellState,
    files: ShellVirtualFileSystem,
    limits: ExecutionLimits,
}

impl<D: CommandDispatcher> Interpreter<D> {
    pub fn new(dispatcher: D) -> Self {
        Self {
            dispatcher,
            state: ShellState::default(),
            files: ShellVirtualFileSystem::default(),
            limits: ExecutionLimits::default(),
        }
    }

    pub fn with_env(
        mut self,
        env: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        self.state.env = env
            .into_iter()
            .map(|(name, value)| (name.into(), value.into()))
            .collect();
        self
    }

    pub fn with_files(mut self, files: ShellVirtualFileSystem) -> Self {
        self.files = files;
        self
    }

    pub fn with_limits(mut self, limits: ExecutionLimits) -> Self {
        self.limits = limits;
        self
    }

    pub fn state(&self) -> &ShellState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut ShellState {
        &mut self.state
    }

    pub fn files(&self) -> &ShellVirtualFileSystem {
        &self.files
    }

    pub fn files_mut(&mut self) -> &mut ShellVirtualFileSystem {
        &mut self.files
    }

    pub fn exec(&mut self, source: &str) -> ExecOutput {
        // Each exec call models a fresh non-interactive shell: functions and
        // aliases defined during the call do not persist to the next exec, even
        // though variables/files on the long-lived session do. Aliases seeded on
        // the session before exec stay available within the call, so snapshot
        // and restore rather than clearing outright.
        let old_functions = std::mem::take(&mut self.state.functions);
        let old_aliases = self.state.aliases.clone();
        self.state.exited = None;
        self.state.returning = None;
        self.state.function_depth = 0;
        self.state.loop_control = None;
        self.state.loop_depth = 0;
        self.state.subshell_depth = 0;
        self.state.errexit = false;
        self.state.errexit_suppressed = 0;
        self.state.command_count = 0;
        self.state.execution_limit = None;
        let mut output = match parse(source) {
            Ok(script) => self.exec_script(&script),
            Err(error) => ExecOutput {
                stdout: String::new(),
                stderr: format!("{error}\n"),
                exit_code: 2,
            },
        };
        // An execution-limit abort overrides the script's running status with a
        // dedicated diagnostic and exit code, mirroring upstream's
        // `ExecutionLimitError` propagation to the top-level result.
        if let Some(message) = self.state.execution_limit.take() {
            output.stderr.push_str(&message);
            output.stderr.push('\n');
            output.exit_code = EXECUTION_LIMIT_EXIT_CODE;
        }
        self.state.functions = old_functions;
        self.state.aliases = old_aliases;
        self.state.exited = None;
        output
    }

    pub fn exec_script(&mut self, script: &Script) -> ExecOutput {
        self.execute_statements(&script.statements)
    }

    fn execute_statements(&mut self, statements: &[Statement]) -> ExecOutput {
        let mut output = ExecOutput::default();
        for statement in statements {
            if self.state.exited.is_some()
                || self.state.loop_control.is_some()
                || self.state.returning.is_some()
                || self.state.execution_limit.is_some()
            {
                break;
            }
            let result = self.execute_statement(statement);
            let status = result.exit_code;
            output.append(result);
            // `set -e`: abort on the first failing statement outside of a
            // tested context. A statement ending in `&&`/`||` is itself a
            // tested compound, so errexit only inspects its final status.
            if self.state.errexit
                && self.state.errexit_suppressed == 0
                && self.state.exited.is_none()
                && self.state.loop_control.is_none()
                && self.state.returning.is_none()
                && status != 0
            {
                self.state.exited = Some(status);
                break;
            }
        }
        if let Some(code) = self.state.exited {
            output.exit_code = code;
        }
        output
    }

    fn execute_statement(&mut self, statement: &Statement) -> ExecOutput {
        // In a `&&`/`||` list, only the final pipeline's status feeds errexit;
        // the intermediate operands form a tested context.
        let is_list = !statement.operators.is_empty();
        if is_list {
            self.state.errexit_suppressed += 1;
        }
        let mut output = self.execute_pipeline(&statement.pipelines[0]);
        for (operator, pipeline) in statement
            .operators
            .iter()
            .zip(statement.pipelines.iter().skip(1))
        {
            let should_execute = match operator {
                ListOperator::And => output.exit_code == 0,
                ListOperator::Or => output.exit_code != 0,
            };
            if should_execute {
                output.append(self.execute_pipeline(pipeline));
            }
        }
        if is_list {
            self.state.errexit_suppressed -= 1;
        }
        self.state.last_status = output.exit_code;
        output
    }

    fn execute_pipeline(&mut self, pipeline: &Pipeline) -> ExecOutput {
        let mut aggregate = ExecOutput::default();
        let mut stdin = String::new();
        let mut statuses = Vec::new();

        // In a multi-command pipeline, every command runs in its own subshell,
        // so an `exit` inside one stage only sets that stage's status and must
        // not terminate the parent script.
        let multi_stage = pipeline.commands.len() > 1;
        for (index, command) in pipeline.commands.iter().enumerate() {
            let command_stdin = std::mem::take(&mut stdin);
            let saved_exited = if multi_stage {
                self.state.exited.take()
            } else {
                None
            };
            let result = self.execute_command(command, command_stdin);
            if multi_stage {
                // Restore the parent's exit state; the stage's own `exit` code
                // is captured below via `result.exit_code`.
                self.state.exited = saved_exited;
            }
            statuses.push(result.exit_code);
            if index + 1 == pipeline.commands.len() {
                aggregate.stdout.push_str(&result.stdout);
                aggregate.stderr.push_str(&result.stderr);
            } else if pipeline.pipe_stderr.get(index).copied().unwrap_or_default() {
                stdin = result.stdout;
                stdin.push_str(&result.stderr);
            } else {
                stdin = result.stdout;
                aggregate.stderr.push_str(&result.stderr);
            }
            if self.state.exited.is_some() {
                break;
            }
        }

        let mut status = if self.state.pipefail {
            statuses
                .iter()
                .rev()
                .find(|status| **status != 0)
                .copied()
                .unwrap_or_else(|| statuses.last().copied().unwrap_or(0))
        } else {
            statuses.last().copied().unwrap_or(0)
        };
        if pipeline.negated {
            status = if status == 0 { 1 } else { 0 };
        }
        aggregate.exit_code = status;
        self.state.last_status = status;
        aggregate
    }

    fn execute_command(&mut self, command: &Command, stdin: String) -> ExecOutput {
        if let Err(message) = self.count_command() {
            // Raise an execution-limit abort that short-circuits the rest of
            // the script (mirroring upstream's propagating `ExecutionLimitError`)
            // rather than letting subsequent commands keep running.
            self.state.execution_limit = Some(message);
            return ExecOutput::default();
        }
        if self.state.execution_limit.is_some() {
            return ExecOutput::default();
        }

        match command {
            Command::Simple(command) => self.execute_simple_command(command, stdin),
            Command::If(command) => self.execute_if(command),
            Command::For(command) => self.execute_for(command),
            Command::ForArith(command) => self.execute_for_arith(command),
            Command::While(command) => self.execute_loop(command, true),
            Command::Until(command) => self.execute_loop(command, false),
            Command::Case(command) => self.execute_case(command),
            Command::FunctionDef(function) => {
                self.state
                    .functions
                    .insert(function.name.clone(), function.clone());
                ExecOutput::default()
            }
            Command::Subshell(body) => {
                let saved_env = self.state.env.clone();
                let saved_arrays = self.state.arrays.clone();
                let saved_xtrace = self.state.xtrace;
                // A subshell runs in a fresh execution context: any enclosing
                // loop is invisible to it, so `break`/`continue` inside the
                // subshell are no-ops and must not unwind the parent loop. An
                // `exit` only terminates the subshell, not the parent script.
                let saved_loop_depth = self.state.loop_depth;
                let saved_loop_control = self.state.loop_control.take();
                let saved_exited = self.state.exited.take();
                self.state.loop_depth = 0;
                self.state.subshell_depth += 1;
                let output = self.execute_statements(body);
                self.state.subshell_depth -= 1;
                self.state.env = saved_env;
                self.state.arrays = saved_arrays;
                self.state.xtrace = saved_xtrace;
                self.state.loop_depth = saved_loop_depth;
                self.state.loop_control = saved_loop_control;
                self.state.exited = saved_exited;
                output
            }
            Command::Group(body) => self.execute_statements(body),
            Command::Arithmetic(expression) => {
                let value = self.eval_arithmetic(&expression.source);
                let status = if value == 0 { 1 } else { 0 };
                self.state.last_status = status;
                ExecOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: status,
                }
            }
            Command::Conditional(expression) => self.execute_conditional(expression),
        }
    }

    fn execute_simple_command(&mut self, command: &SimpleCommand, stdin: String) -> ExecOutput {
        let assignments = self.expand_assignments(&command.assignments);
        if let Some(output) = self.take_arith_error() {
            return output;
        }
        let Some(name_word) = &command.name else {
            let trace = self.trace_simple_command(&assignments, None);
            for assignment in assignments {
                self.apply_assignment(assignment);
            }
            return prepend_trace(ExecOutput::default(), trace);
        };

        let Some(name) = self.expand_word(name_word, true).into_iter().next() else {
            return ExecOutput::default();
        };
        let args = command
            .args
            .iter()
            .flat_map(|word| self.expand_word(word, true))
            .collect::<Vec<_>>();
        if let Some(output) = self.take_arith_error() {
            return output;
        }
        let redirections = command
            .redirections
            .iter()
            .map(|redirection| AppliedRedirection {
                fd: redirection.fd,
                operator: redirection.operator,
                target: self.expand_word_to_string(&redirection.target),
                here_doc: redirection.here_doc.clone(),
            })
            .collect::<Vec<_>>();
        let trace = self.trace_simple_command(&assignments, Some((&name, &args)));

        if let Some(alias) = self.state.aliases.get(&name).map(str::to_string)
            && self.state.alias_depth < 20
        {
            self.state.alias_depth += 1;
            let source = alias_command_source(&alias, &args);
            let output = match parse(&source) {
                Ok(script) => self.exec_script(&script),
                Err(error) => ExecOutput {
                    stdout: String::new(),
                    stderr: format!("{error}\n"),
                    exit_code: 2,
                },
            };
            self.state.alias_depth -= 1;
            return prepend_trace(output, trace);
        }

        if let Some(function) = self.state.functions.get(&name).cloned() {
            return prepend_trace(self.call_function(function, args), trace);
        }

        if let Some(output) = self.execute_builtin(&name, &args) {
            return prepend_trace(self.apply_redirections(output, &redirections), trace);
        }

        let mut env = self.state.env.clone();
        for assignment in assignments {
            match assignment {
                ExpandedAssignment::Scalar { name, value, .. } => {
                    env.insert(name, value);
                }
                ExpandedAssignment::Array { .. } | ExpandedAssignment::ArrayElement { .. } => {}
            }
        }

        let prepared_stdin = self.apply_input_redirections(stdin, &redirections);
        let invocation = CommandInvocation {
            name,
            args,
            stdin: prepared_stdin,
            env,
            redirections: redirections.clone(),
        };
        let result = self.dispatcher.dispatch(invocation, &mut self.files);
        prepend_trace(
            self.apply_redirections(
                ExecOutput {
                    stdout: result.stdout,
                    stderr: result.stderr,
                    exit_code: result.exit_code,
                },
                &redirections,
            ),
            trace,
        )
    }

    fn trace_simple_command(
        &self,
        assignments: &[ExpandedAssignment],
        command: Option<(&str, &[String])>,
    ) -> Option<String> {
        if !self.state.xtrace {
            return None;
        }

        let mut parts = assignments
            .iter()
            .filter_map(format_expanded_assignment)
            .collect::<Vec<_>>();
        if let Some((name, args)) = command {
            parts.push(trace_quote_arg(name));
            parts.extend(args.iter().map(|arg| trace_quote_arg(arg)));
        }
        if parts.is_empty() {
            return None;
        }

        Some(format!("{}{}\n", self.trace_prefix(), parts.join(" ")))
    }

    fn trace_prefix(&self) -> String {
        let raw = self.state.lookup_var("PS4").unwrap_or("+ ");
        expand_trace_prefix(raw, &self.state)
    }

    fn apply_set_options(&mut self, args: &[String]) -> ExecOutput {
        // `set --help` prints the usage text and succeeds (mirrors upstream
        // `handleSet`'s `success(SET_USAGE)`).
        if args.iter().any(|arg| arg == "--help") {
            return ExecOutput {
                stdout: SET_USAGE.to_string(),
                stderr: String::new(),
                exit_code: 0,
            };
        }

        if args == ["-o", "pipefail"] {
            self.state.pipefail = true;
            return ExecOutput::default();
        }
        if args == ["+o", "pipefail"] {
            self.state.pipefail = false;
            return ExecOutput::default();
        }
        if args == ["-o", "errexit"] {
            self.state.errexit = true;
            return ExecOutput::default();
        }
        if args == ["+o", "errexit"] {
            self.state.errexit = false;
            return ExecOutput::default();
        }

        // `set -o` / `set +o` with no option name lists the current option
        // settings (upstream prints implemented + no-op options, sorted).
        if args == ["-o"] {
            return ExecOutput {
                stdout: self.format_set_o_listing(false),
                stderr: String::new(),
                exit_code: 0,
            };
        }
        if args == ["+o"] {
            return ExecOutput {
                stdout: self.format_set_o_listing(true),
                stderr: String::new(),
                exit_code: 0,
            };
        }

        let mut index = 0;
        while index < args.len() {
            let arg = args[index].as_str();

            // `-o <name>` / `+o <name>`: a long option name. Unknown names are
            // an "invalid option name" error (exit 1, usage to stderr).
            if (arg == "-o" || arg == "+o")
                && args
                    .get(index + 1)
                    .is_some_and(|next| !next.starts_with('-') && !next.starts_with('+'))
            {
                let name = args[index + 1].as_str();
                let enable = arg == "-o";
                match name {
                    "errexit" => self.state.errexit = enable,
                    "pipefail" => self.state.pipefail = enable,
                    "xtrace" => self.state.xtrace = enable,
                    // Accepted long option names that are no-ops here.
                    "nounset"
                    | "verbose"
                    | "noclobber"
                    | "noglob"
                    | "allexport"
                    | "noexec"
                    | "posix"
                    | "vi"
                    | "emacs"
                    | "notify"
                    | "monitor"
                    | "braceexpand"
                    | "histexpand"
                    | "physical"
                    | "functrace"
                    | "errtrace"
                    | "privileged"
                    | "hashall"
                    | "ignoreeof"
                    | "interactive-comments"
                    | "keyword"
                    | "onecmd" => {}
                    other => {
                        return ExecOutput {
                            stdout: String::new(),
                            stderr: format!("bash: set: {other}: invalid option name\n{SET_USAGE}"),
                            exit_code: 1,
                        };
                    }
                }
                index += 2;
                continue;
            }

            // Combined short flags like `-eu`, `+x`, `-z`. Unknown short flags
            // are an "invalid option" error.
            if arg.len() > 1
                && (arg.starts_with('-') || arg.starts_with('+'))
                && !arg.starts_with("--")
            {
                let enable = arg.starts_with('-');
                let sign = if enable { '-' } else { '+' };
                for flag in arg[1..].chars() {
                    match flag {
                        'e' => self.state.errexit = enable,
                        'x' => self.state.xtrace = enable,
                        // Accepted short flags that are no-ops here.
                        'u' | 'v' | 'f' | 'C' | 'a' | 'n' | 'h' | 'b' | 'm' | 'B' | 'H' | 'P'
                        | 'T' | 'E' | 'p' => {}
                        other => {
                            return ExecOutput {
                                stdout: String::new(),
                                stderr: format!(
                                    "bash: set: {sign}{other}: invalid option\n{SET_USAGE}"
                                ),
                                exit_code: 1,
                            };
                        }
                    }
                }
                index += 1;
                continue;
            }

            index += 1;
        }
        ExecOutput::default()
    }

    /// Render the `set -o` (`list=false` ⇒ leading `+o` form when `commands`)
    /// option listing. When `commands` is true the `set +o` form prints
    /// `set -o`/`set +o` lines; otherwise it prints `name  on/off` columns.
    fn format_set_o_listing(&self, commands: bool) -> String {
        // (name, current value). Order is alphabetised below to match bash.
        let implemented: [(&str, bool); 5] = [
            ("errexit", self.state.errexit),
            ("nounset", false),
            ("pipefail", self.state.pipefail),
            ("verbose", false),
            ("xtrace", self.state.xtrace),
        ];
        let noop = [
            "allexport",
            "braceexpand",
            "emacs",
            "errtrace",
            "functrace",
            "hashall",
            "histexpand",
            "ignoreeof",
            "interactive-comments",
            "keyword",
            "monitor",
            "noclobber",
            "noexec",
            "noglob",
            "notify",
            "onecmd",
            "physical",
            "posix",
            "privileged",
            "vi",
        ];
        let mut lines: Vec<String> = Vec::new();
        if commands {
            for (name, on) in implemented {
                lines.push(format!("set {} {name}", if on { "-o" } else { "+o" }));
            }
            for name in noop {
                lines.push(format!("set +o {name}"));
            }
        } else {
            for (name, on) in implemented {
                lines.push(format!("{:<16}{}", name, if on { "on" } else { "off" }));
            }
            for name in noop {
                lines.push(format!("{name:<16}off"));
            }
        }
        lines.sort();
        let mut out = lines.join("\n");
        out.push('\n');
        out
    }

    fn expand_assignments(&mut self, assignments: &[Assignment]) -> Vec<ExpandedAssignment> {
        assignments
            .iter()
            .map(|assignment| {
                if let Some(array) = &assignment.array {
                    ExpandedAssignment::Array {
                        name: assignment.name.clone(),
                        values: array
                            .iter()
                            .map(|word| self.expand_word_to_string(word))
                            .collect(),
                    }
                } else if let Some(index) = assignment.index {
                    ExpandedAssignment::ArrayElement {
                        name: assignment.name.clone(),
                        index,
                        value: assignment
                            .value
                            .as_ref()
                            .map_or_else(String::new, |word| self.expand_word_to_string(word)),
                        append: assignment.append,
                    }
                } else {
                    ExpandedAssignment::Scalar {
                        name: assignment.name.clone(),
                        value: assignment
                            .value
                            .as_ref()
                            .map_or_else(String::new, |word| self.expand_word_to_string(word)),
                        append: assignment.append,
                    }
                }
            })
            .collect()
    }

    fn apply_assignment(&mut self, assignment: ExpandedAssignment) {
        match assignment {
            ExpandedAssignment::Scalar {
                name,
                value,
                append,
            } => {
                if append {
                    let mut previous = self.lookup_parameter(&name);
                    previous.push_str(&value);
                    self.state.assign_var(name, previous);
                } else {
                    self.state.assign_var(name, value);
                }
            }
            ExpandedAssignment::Array { name, values } => {
                self.state.arrays.insert(name, values);
            }
            ExpandedAssignment::ArrayElement {
                name,
                index,
                value,
                append,
            } => {
                let values = self.state.arrays.entry(name).or_default();
                if values.len() <= index {
                    values.resize(index + 1, String::new());
                }
                if append {
                    values[index].push_str(&value);
                } else {
                    values[index] = value;
                }
            }
        }
    }

    fn execute_builtin(&mut self, name: &str, args: &[String]) -> Option<ExecOutput> {
        match name {
            ":" | "true" => Some(ExecOutput::default()),
            "false" => Some(ExecOutput {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 1,
            }),
            "export" => Some(self.execute_export(args)),
            "unset" => Some(self.execute_unset(args)),
            "local" => Some(self.execute_local(args)),
            "declare" | "typeset" => Some(self.execute_declare(args)),
            "set" => Some(self.apply_set_options(args)),
            "alias" => Some(self.execute_alias(args)),
            "unalias" => Some(self.execute_unalias(args)),
            "exit" => {
                // With no argument, `exit` resolves to the most recent command's
                // status (bash semantics). With an argument, parse it and wrap
                // modulo 256; a non-numeric argument is a status-2 error (the
                // shell still exits, like bash).
                let (code, stderr) = match args.first() {
                    None => (self.state.last_status, String::new()),
                    Some(arg) => match arg.parse::<i64>() {
                        Ok(value) => (value.rem_euclid(256) as i32, String::new()),
                        Err(_) => (2, format!("bash: exit: {arg}: numeric argument required\n")),
                    },
                };
                self.state.exited = Some(code);
                Some(ExecOutput {
                    stdout: String::new(),
                    stderr,
                    exit_code: code,
                })
            }
            "break" | "continue" => Some(self.execute_loop_control(name, args)),
            "return" => Some(self.execute_return(args)),
            "shift" => Some(self.execute_shift(args)),
            "eval" => Some(self.execute_eval(args)),
            _ => None,
        }
    }

    /// `return [n]` builtin. Outside a function (and outside a sourced script,
    /// which Just Bash does not model) it is an error. With no argument it
    /// resolves to the last command's status; otherwise it parses `n` and wraps
    /// it modulo 256 (matching `exit`). A non-numeric argument is a status-2
    /// error and does not raise the return signal.
    fn execute_return(&mut self, args: &[String]) -> ExecOutput {
        if self.state.function_depth == 0 {
            return ExecOutput {
                stdout: String::new(),
                stderr: "bash: return: can only `return' from a function or sourced script\n"
                    .to_string(),
                exit_code: 1,
            };
        }
        let code = match args.first() {
            None => self.state.last_status,
            Some(arg) => match arg.parse::<i64>() {
                Ok(value) => value.rem_euclid(256) as i32,
                Err(_) => {
                    return ExecOutput {
                        stdout: String::new(),
                        stderr: format!("bash: return: {arg}: numeric argument required\n"),
                        exit_code: 2,
                    };
                }
            },
        };
        self.state.returning = Some(code);
        ExecOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: code,
        }
    }

    /// `shift [n]` builtin. Shifts the positional parameters left by `n`
    /// (default 1): `$(n+1)` becomes `$1`, and `$#` is decremented by `n`.
    /// A non-numeric or negative argument is a "numeric argument required"
    /// error (status 1); a count greater than the current parameter count is
    /// a "shift count out of range" error (status 1). `shift 0` is a no-op.
    /// (Mirrors `interpreter/builtins/shift.ts`; the POSIX-mode fatal variant
    /// is not modeled because Just Bash does not track `set -o posix`.)
    fn execute_shift(&mut self, args: &[String]) -> ExecOutput {
        let mut n: usize = 1;
        if let Some(arg) = args.first() {
            match arg.parse::<i64>() {
                Ok(value) if value >= 0 => n = value as usize,
                _ => {
                    return ExecOutput {
                        stdout: String::new(),
                        stderr: format!("bash: shift: {arg}: numeric argument required\n"),
                        exit_code: 1,
                    };
                }
            }
        }

        if n > self.state.positionals.len() {
            return ExecOutput {
                stdout: String::new(),
                stderr: "bash: shift: shift count out of range\n".to_string(),
                exit_code: 1,
            };
        }

        if n == 0 {
            return ExecOutput::default();
        }

        self.state.positionals.drain(0..n);
        ExecOutput::default()
    }

    /// `eval [arg ...]` builtin. The arguments are joined with single spaces,
    /// re-parsed, and executed in the current environment. An empty program (no
    /// args or an empty string) is a success no-op. A parse error reports
    /// "Parse error" on stderr and resolves to status 1.
    fn execute_eval(&mut self, args: &[String]) -> ExecOutput {
        let program = args.join(" ");
        if program.trim().is_empty() {
            return ExecOutput::default();
        }
        match parse(&program) {
            Ok(script) => self.exec_script(&script),
            Err(error) => ExecOutput {
                stdout: String::new(),
                stderr: format!("bash: eval: Parse error: {error}\n"),
                exit_code: 1,
            },
        }
    }

    fn execute_loop_control(&mut self, name: &str, args: &[String]) -> ExecOutput {
        // Outside any loop, break/continue are silent no-ops at the top level.
        // Inside a subshell with no enclosing loop, bash terminates the subshell
        // (the builtin acts like a local `exit`) without touching the parent.
        // This loop-depth check happens BEFORE any argument validation, matching
        // bash (and the upstream break/continue builtins).
        if self.state.loop_depth == 0 {
            if self.state.subshell_depth > 0 {
                self.state.exited = Some(self.state.last_status);
            }
            return ExecOutput::default();
        }

        // bash: too many arguments is a fatal error (like `exit 1`) for both
        // `break` and `continue`; it aborts the entire script.
        if args.len() > 1 {
            self.state.exited = Some(1);
            return ExecOutput {
                stdout: String::new(),
                stderr: format!("bash: {name}: too many arguments\n"),
                exit_code: 1,
            };
        }

        // Parse the optional numeric level argument. Bash defaults to 1. An
        // invalid (non-numeric or < 1) argument is fatal: `break` exits with
        // status 128, `continue` exits with status 1.
        let invalid_exit = if name == "break" { 128 } else { 1 };
        let levels = match args.first() {
            None => 1u32,
            Some(arg) => match arg.parse::<i64>() {
                Ok(value) if value >= 1 => value as u32,
                _ => {
                    // A numeric-argument error is fatal (like `exit`) and aborts
                    // the script: `break` exits 128, `continue` exits 1.
                    self.state.exited = Some(invalid_exit);
                    return ExecOutput {
                        stdout: String::new(),
                        stderr: format!("bash: {name}: {arg}: numeric argument required\n"),
                        exit_code: invalid_exit,
                    };
                }
            },
        };

        let levels = levels.min(self.state.loop_depth);
        self.state.loop_control = Some(match name {
            "break" => LoopControl::Break(levels),
            _ => LoopControl::Continue(levels),
        });
        ExecOutput::default()
    }

    fn execute_local(&mut self, args: &[String]) -> ExecOutput {
        if self.state.local_scopes.is_empty() {
            return ExecOutput {
                stdout: String::new(),
                stderr: "local: can only be used in a function\n".to_string(),
                exit_code: 1,
            };
        }
        for arg in args {
            if let Some((name, value)) = split_assignment_text(arg) {
                self.state.declare_local(name, Some(value));
            } else {
                self.state.declare_local(arg.clone(), None);
            }
        }
        ExecOutput::default()
    }

    fn execute_export(&mut self, args: &[String]) -> ExecOutput {
        if args.is_empty() || args == ["-p"] {
            return ExecOutput {
                stdout: format_export_listing(&self.state.env),
                stderr: String::new(),
                exit_code: 0,
            };
        }

        let mut no_export_attribute = false;
        for arg in args {
            match arg.as_str() {
                "-n" => {
                    no_export_attribute = true;
                }
                "-p" => {}
                _ if no_export_attribute && is_valid_name(arg) => {}
                _ => {
                    if let Some((name, value)) = split_assignment_text(arg) {
                        self.state.assign_var(name, value);
                    } else if is_valid_name(arg) {
                        let value = self.state.lookup_var(arg).unwrap_or_default().to_string();
                        self.state.env.entry(arg.clone()).or_insert(value);
                    }
                }
            }
        }

        ExecOutput::default()
    }

    fn execute_unset(&mut self, args: &[String]) -> ExecOutput {
        let mut unset_functions = false;
        for arg in args {
            match arg.as_str() {
                "-f" => unset_functions = true,
                "-v" => unset_functions = false,
                _ if unset_functions => {
                    self.state.functions.remove(arg);
                }
                _ => self.state.unset_var(arg),
            }
        }
        ExecOutput::default()
    }

    fn execute_declare(&mut self, args: &[String]) -> ExecOutput {
        let mut declare_array = false;
        for arg in args {
            match arg.as_str() {
                "-a" | "-A" => declare_array = true,
                _ if declare_array && is_valid_name(arg) => {
                    self.state.arrays.entry(arg.clone()).or_default();
                }
                _ if is_valid_name(arg) => {
                    self.state.env.entry(arg.clone()).or_default();
                }
                _ => {
                    if let Some((name, value)) = split_assignment_text(arg) {
                        self.state.assign_var(name, value);
                    }
                }
            }
        }
        ExecOutput::default()
    }

    fn execute_alias(&mut self, args: &[String]) -> ExecOutput {
        if args.iter().any(|arg| arg == "--help") {
            return ExecOutput {
                stdout: alias_help_text(),
                stderr: String::new(),
                exit_code: 0,
            };
        }

        // No arguments: list all aliases in definition order.
        if args.is_empty() {
            let stdout = self
                .state
                .aliases
                .iter()
                .map(|(name, value)| format!("alias {name}='{value}'\n"))
                .collect();
            return ExecOutput {
                stdout,
                stderr: String::new(),
                exit_code: 0,
            };
        }

        // Skip the "--" option separator (POSIX standard).
        let process_args: &[String] = if args.first().map(String::as_str) == Some("--") {
            &args[1..]
        } else {
            args
        };

        for arg in process_args {
            if let Some((name, value)) = split_alias_assignment(arg) {
                self.state.aliases.insert(name, value);
            } else if let Some(value) = self.state.aliases.get(arg) {
                return ExecOutput {
                    stdout: format!("alias {arg}='{value}'\n"),
                    stderr: String::new(),
                    exit_code: 0,
                };
            } else {
                return ExecOutput {
                    stdout: String::new(),
                    stderr: format!("alias: {arg}: not found\n"),
                    exit_code: 1,
                };
            }
        }

        ExecOutput::default()
    }

    fn execute_unalias(&mut self, args: &[String]) -> ExecOutput {
        if args.iter().any(|arg| arg == "--help") {
            return ExecOutput {
                stdout: unalias_help_text(),
                stderr: String::new(),
                exit_code: 0,
            };
        }

        if args.is_empty() {
            return ExecOutput {
                stdout: String::new(),
                stderr: "unalias: usage: unalias [-a] name [name ...]\n".to_string(),
                exit_code: 1,
            };
        }

        if args.first().map(String::as_str) == Some("-a") {
            self.state.aliases.clear();
            return ExecOutput::default();
        }

        let mut stderr = String::new();
        let mut exit_code = 0;
        for name in args {
            if !self.state.aliases.remove(name) {
                stderr.push_str(&format!("unalias: {name}: not found\n"));
                exit_code = 1;
            }
        }

        ExecOutput {
            stdout: String::new(),
            stderr,
            exit_code,
        }
    }

    fn call_function(&mut self, function: FunctionDef, args: Vec<String>) -> ExecOutput {
        // Guard against runaway recursion before pushing a new call frame.
        // Mirrors upstream `callFunction`: the depth counter is incremented, and
        // if it exceeds `maxCallDepth` an execution-limit abort is raised that
        // short-circuits the rest of the script instead of overflowing the
        // native stack.
        if self.state.function_depth as usize >= self.limits.max_call_depth {
            self.state.execution_limit = Some(format!(
                "{}: maximum recursion depth ({}) exceeded, increase executionLimits.maxCallDepth",
                function.name, self.limits.max_call_depth
            ));
            return ExecOutput::default();
        }
        let saved_positionals = std::mem::replace(&mut self.state.positionals, args);
        self.state.local_scopes.push(BTreeMap::new());
        self.state.function_depth += 1;
        let mut output = self.execute_command(&function.body, String::new());
        self.state.function_depth -= 1;
        // A `return` raised inside this function stops at its own frame: consume
        // the pending signal and resolve the call's status to the returned code.
        if let Some(code) = self.state.returning.take() {
            output.exit_code = code;
        }
        self.state.local_scopes.pop();
        self.state.positionals = saved_positionals;
        output
    }

    fn execute_if(&mut self, command: &IfCommand) -> ExecOutput {
        let mut output = ExecOutput::default();
        for clause in &command.clauses {
            self.state.errexit_suppressed += 1;
            let condition = self.execute_statements(&clause.condition);
            self.state.errexit_suppressed -= 1;
            let condition_status = condition.exit_code;
            output.append(condition);
            if condition_status == 0 {
                output.append(self.execute_statements(&clause.body));
                return output;
            }
        }
        if !command.else_body.is_empty() {
            output.append(self.execute_statements(&command.else_body));
        } else {
            output.exit_code = 0;
        }
        output
    }

    fn execute_for(&mut self, command: &ForCommand) -> ExecOutput {
        let mut output = ExecOutput::default();
        if !is_valid_name(&command.variable) {
            output.stderr.push_str(&format!(
                "bash: `{}': not a valid identifier\n",
                command.variable
            ));
            output.exit_code = 1;
            return output;
        }
        let mut iterations = 0usize;
        let words = if command.has_in_clause {
            command
                .words
                .iter()
                .flat_map(|word| self.expand_word(word, true))
                .collect::<Vec<_>>()
        } else {
            // `for i; do ... done` iterates over positional parameters ($@).
            self.positional_parameters()
        };
        self.state.loop_depth += 1;
        for value in words {
            iterations += 1;
            if iterations > self.limits.max_loop_iterations {
                self.state.execution_limit = Some(format!(
                    "for loop: too many iterations ({}), increase executionLimits.maxLoopIterations",
                    self.limits.max_loop_iterations
                ));
                break;
            }
            self.state.assign_var(command.variable.clone(), value);
            output.append(self.execute_statements(&command.body));
            if self.state.exited.is_some()
                || self.state.returning.is_some()
                || self.state.execution_limit.is_some()
            {
                break;
            }
            match self.consume_loop_control() {
                LoopFlow::Continue => {}
                LoopFlow::BreakLoop => break,
            }
        }
        self.state.loop_depth -= 1;
        output
    }

    /// Positional parameters for `for i; do ... done`. Mirrors upstream
    /// `control-flow.ts`: the `@` environment variable is split on spaces with
    /// empty fields dropped.
    fn positional_parameters(&self) -> Vec<String> {
        if !self.state.positionals.is_empty() {
            return self.state.positionals.clone();
        }
        self.state
            .get_var("@")
            .unwrap_or("")
            .split(' ')
            .filter(|field| !field.is_empty())
            .map(str::to_string)
            .collect()
    }

    /// Execute a C-style `for (( init; cond; update ))` loop.
    fn execute_for_arith(&mut self, command: &ForArithCommand) -> ExecOutput {
        let mut output = ExecOutput::default();
        if !command.init.trim().is_empty() {
            self.eval_arithmetic(&command.init);
        }
        let mut iterations = 0usize;
        self.state.loop_depth += 1;
        loop {
            // An empty condition is treated as always true (infinite loop).
            let keep_going = if command.condition.trim().is_empty() {
                true
            } else {
                self.eval_arithmetic(&command.condition) != 0
            };
            if !keep_going {
                output.exit_code = 0;
                break;
            }
            iterations += 1;
            if iterations > self.limits.max_loop_iterations {
                self.state.execution_limit = Some(format!(
                    "for loop: too many iterations ({}), increase executionLimits.maxLoopIterations",
                    self.limits.max_loop_iterations
                ));
                break;
            }
            output.append(self.execute_statements(&command.body));
            if self.state.exited.is_some() || self.state.execution_limit.is_some() {
                break;
            }
            match self.consume_loop_control() {
                LoopFlow::Continue => {}
                LoopFlow::BreakLoop => break,
            }
            if !command.update.trim().is_empty() {
                self.eval_arithmetic(&command.update);
            }
        }
        self.state.loop_depth -= 1;
        output
    }

    /// Resolve any pending `break`/`continue` signal for the loop currently
    /// unwinding. Returns whether this loop should break out of its own
    /// iteration cycle. Multi-level signals are decremented and re-raised so
    /// the enclosing loop continues unwinding.
    fn consume_loop_control(&mut self) -> LoopFlow {
        match self.state.loop_control {
            None => LoopFlow::Continue,
            Some(LoopControl::Break(1)) => {
                self.state.loop_control = None;
                LoopFlow::BreakLoop
            }
            Some(LoopControl::Break(levels)) => {
                self.state.loop_control = Some(LoopControl::Break(levels - 1));
                LoopFlow::BreakLoop
            }
            Some(LoopControl::Continue(1)) => {
                self.state.loop_control = None;
                LoopFlow::Continue
            }
            Some(LoopControl::Continue(levels)) => {
                // This loop is not the target: keep unwinding via a break, but
                // re-raise as a continue for the parent loop.
                self.state.loop_control = Some(LoopControl::Continue(levels - 1));
                LoopFlow::BreakLoop
            }
        }
    }

    fn execute_loop(&mut self, command: &LoopCommand, is_while: bool) -> ExecOutput {
        let mut output = ExecOutput::default();
        self.state.loop_depth += 1;
        for _ in 0..self.limits.max_loop_iterations {
            self.state.errexit_suppressed += 1;
            let condition = self.execute_statements(&command.condition);
            self.state.errexit_suppressed -= 1;
            let condition_status = condition.exit_code;
            output.append(condition);
            let should_run = if is_while {
                condition_status == 0
            } else {
                condition_status != 0
            };
            if !should_run {
                output.exit_code = 0;
                self.state.loop_depth -= 1;
                return output;
            }
            output.append(self.execute_statements(&command.body));
            if self.state.exited.is_some()
                || self.state.returning.is_some()
                || self.state.execution_limit.is_some()
            {
                self.state.loop_depth -= 1;
                return output;
            }
            match self.consume_loop_control() {
                LoopFlow::Continue => {}
                LoopFlow::BreakLoop => {
                    self.state.loop_depth -= 1;
                    return output;
                }
            }
        }
        self.state.loop_depth -= 1;
        let loop_kind = if is_while { "while" } else { "until" };
        self.state.execution_limit = Some(format!(
            "{loop_kind} loop: too many iterations ({}), increase executionLimits.maxLoopIterations",
            self.limits.max_loop_iterations
        ));
        output
    }

    fn execute_case(&mut self, command: &CaseCommand) -> ExecOutput {
        let value = self.expand_word_to_string(&command.word);
        let mut output = ExecOutput::default();
        let mut index = 0;
        while index < command.items.len() {
            let item = &command.items[index];
            let matched = item.patterns.iter().any(|pattern| {
                let pattern = self.expand_word_to_string(pattern);
                pattern_matches(&pattern, &value)
            });
            if !matched {
                index += 1;
                continue;
            }
            output.append(self.execute_statements(&item.body));
            match item.terminator {
                CaseTerminator::Break => return output,
                CaseTerminator::ContinueMatching => {
                    // Resume testing patterns from the next clause.
                    index += 1;
                }
                CaseTerminator::FallThrough => {
                    // Run subsequent clause bodies unconditionally until a
                    // non-fall-through terminator is reached.
                    index += 1;
                    while index < command.items.len() {
                        let next = &command.items[index];
                        output.append(self.execute_statements(&next.body));
                        match next.terminator {
                            CaseTerminator::FallThrough => index += 1,
                            CaseTerminator::ContinueMatching => {
                                index += 1;
                                break;
                            }
                            CaseTerminator::Break => return output,
                        }
                    }
                }
            }
        }
        output
    }

    fn execute_conditional(&mut self, expression: &str) -> ExecOutput {
        let tokens = shell_words(expression);
        let token_refs = tokens.iter().map(String::as_str).collect::<Vec<_>>();
        let status = match token_refs.as_slice() {
            ["-n", value] => i32::from(value.is_empty()),
            ["-z", value] => i32::from(!value.is_empty()),
            [left, operator, right] => {
                let left = self.expand_inline_text(left);
                let right = self.expand_inline_text(right);
                let matched = match *operator {
                    "=" | "==" => left == right,
                    "!=" => left != right,
                    "-eq" => parse_i64(&left) == parse_i64(&right),
                    "-ne" => parse_i64(&left) != parse_i64(&right),
                    "-lt" => parse_i64(&left) < parse_i64(&right),
                    "-le" => parse_i64(&left) <= parse_i64(&right),
                    "-gt" => parse_i64(&left) > parse_i64(&right),
                    "-ge" => parse_i64(&left) >= parse_i64(&right),
                    _ => false,
                };
                if matched { 0 } else { 1 }
            }
            [word] => {
                if self.expand_inline_text(word).is_empty() {
                    1
                } else {
                    0
                }
            }
            _ => 1,
        };
        ExecOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: status,
        }
    }

    fn apply_input_redirections(
        &mut self,
        mut stdin: String,
        redirections: &[AppliedRedirection],
    ) -> String {
        for redirection in redirections {
            match redirection.operator {
                RedirectionOperator::Input => {
                    stdin = self
                        .files
                        .read_to_string(&redirection.target)
                        .unwrap_or_default()
                        .to_string();
                }
                RedirectionOperator::HereString => {
                    stdin = format!("{}\n", redirection.target);
                }
                RedirectionOperator::HereDoc | RedirectionOperator::HereDocStripTabs => {
                    if let Some(here_doc) = &redirection.here_doc {
                        stdin = self.expand_here_doc(here_doc);
                    }
                }
                _ => {}
            }
        }
        stdin
    }

    fn apply_redirections(
        &mut self,
        mut output: ExecOutput,
        redirections: &[AppliedRedirection],
    ) -> ExecOutput {
        for redirection in redirections {
            let fd = redirection.fd.unwrap_or(match redirection.operator {
                RedirectionOperator::Input
                | RedirectionOperator::DuplicateInput
                | RedirectionOperator::ReadWrite
                | RedirectionOperator::HereDoc
                | RedirectionOperator::HereDocStripTabs => 0,
                _ => 1,
            });
            match redirection.operator {
                RedirectionOperator::Output | RedirectionOperator::Clobber if fd == 1 => {
                    write_or_discard(&mut self.files, &redirection.target, &output.stdout, false);
                    output.stdout.clear();
                }
                RedirectionOperator::Append if fd == 1 => {
                    write_or_discard(&mut self.files, &redirection.target, &output.stdout, true);
                    output.stdout.clear();
                }
                RedirectionOperator::Output | RedirectionOperator::Clobber if fd == 2 => {
                    write_or_discard(&mut self.files, &redirection.target, &output.stderr, false);
                    output.stderr.clear();
                }
                RedirectionOperator::Append if fd == 2 => {
                    write_or_discard(&mut self.files, &redirection.target, &output.stderr, true);
                    output.stderr.clear();
                }
                RedirectionOperator::DuplicateOutput if fd == 2 && redirection.target == "1" => {
                    output.stdout.push_str(&output.stderr);
                    output.stderr.clear();
                }
                RedirectionOperator::OutputBoth => {
                    let combined = format!("{}{}", output.stdout, output.stderr);
                    write_or_discard(&mut self.files, &redirection.target, &combined, false);
                    output.stdout.clear();
                    output.stderr.clear();
                }
                RedirectionOperator::AppendBoth => {
                    let combined = format!("{}{}", output.stdout, output.stderr);
                    write_or_discard(&mut self.files, &redirection.target, &combined, true);
                    output.stdout.clear();
                    output.stderr.clear();
                }
                _ => {}
            }
        }
        output
    }

    fn expand_here_doc(&mut self, here_doc: &HereDoc) -> String {
        let content = if here_doc.strip_tabs {
            strip_leading_tabs_from_lines(&here_doc.content)
        } else {
            here_doc.content.clone()
        };
        if here_doc.quoted {
            content
        } else {
            self.expand_here_doc_unquoted(&content)
        }
    }

    fn expand_here_doc_unquoted(&mut self, content: &str) -> String {
        let chars = content.chars().collect::<Vec<_>>();
        let mut output = String::new();
        let mut index = 0usize;
        while index < chars.len() {
            match chars[index] {
                '\\' => {
                    if let Some(next) = chars.get(index + 1).copied()
                        && matches!(next, '$' | '`' | '\\')
                    {
                        output.push(next);
                        index += 2;
                    } else {
                        output.push('\\');
                        index += 1;
                    }
                }
                '$' => {
                    let (expanded, next_index) = self.expand_here_doc_dollar(&chars, index);
                    output.push_str(&expanded);
                    index = next_index;
                }
                '`' => {
                    if let Some(end) = chars[index + 1..]
                        .iter()
                        .position(|character| *character == '`')
                    {
                        let body = chars[index + 1..index + 1 + end].iter().collect::<String>();
                        output.push_str(&self.execute_substitution_source(&body));
                        index += end + 2;
                    } else {
                        output.push('`');
                        index += 1;
                    }
                }
                character => {
                    output.push(character);
                    index += 1;
                }
            }
        }
        output
    }

    fn expand_here_doc_dollar(&mut self, chars: &[char], index: usize) -> (String, usize) {
        if chars.get(index + 1) == Some(&'(') && chars.get(index + 2) == Some(&'(') {
            if let Some(end) = find_double_right_paren(chars, index + 3) {
                let source = chars[index + 3..end].iter().collect::<String>();
                return (self.eval_arithmetic(&source).to_string(), end + 2);
            }
        }

        if chars.get(index + 1) == Some(&'(') {
            if let Some(end) = find_matching_substitution_paren(chars, index + 2) {
                let body = chars[index + 2..end].iter().collect::<String>();
                return (self.execute_substitution_source(&body), end + 1);
            }
        }

        if chars.get(index + 1) == Some(&'{') {
            if let Some(end) = chars[index + 2..]
                .iter()
                .position(|character| *character == '}')
            {
                let content = chars[index + 2..index + 2 + end].iter().collect::<String>();
                let parameter = parse_parameter_expansion(&content);
                return (
                    self.expand_parameter(&parameter, false).join(" "),
                    index + end + 3,
                );
            }
        }

        if let Some(parameter) = chars.get(index + 1).copied() {
            if is_special_parameter(parameter) {
                return (self.lookup_parameter(&parameter.to_string()), index + 2);
            }
            if is_name_start(parameter) {
                let mut end = index + 2;
                while chars
                    .get(end)
                    .is_some_and(|character| is_name_continue(*character))
                {
                    end += 1;
                }
                let name = chars[index + 1..end].iter().collect::<String>();
                return (self.lookup_parameter(&name), end);
            }
        }

        ("$".to_string(), index + 1)
    }

    fn execute_substitution_source(&mut self, source: &str) -> String {
        let output = match parse(source) {
            Ok(script) => self.exec_script(&script).stdout,
            Err(_) => String::new(),
        };
        output.trim_end_matches('\n').replace('\n', " ")
    }

    fn count_command(&mut self) -> Result<(), String> {
        self.state.command_count += 1;
        if self.state.command_count > self.limits.max_commands {
            Err(format!(
                "too many commands executed (>{}), increase executionLimits.maxCommandCount",
                self.limits.max_commands
            ))
        } else {
            Ok(())
        }
    }

    fn expand_word(&mut self, word: &Word, split: bool) -> Vec<String> {
        let preserve_whitespace = word.preserves_whitespace();
        let mut values = vec![String::new()];
        for part in &word.parts {
            let part_values = self.expand_part(part, false);
            values = append_expanded_values(values, part_values);
        }

        if split && !preserve_whitespace {
            let ifs = self.state.get_var("IFS").map(str::to_string);
            values
                .into_iter()
                .flat_map(|value| split_on_ifs(&value, ifs.as_deref()))
                .collect()
        } else {
            values
        }
    }

    fn expand_word_to_string(&mut self, word: &Word) -> String {
        self.expand_word(word, false).join(" ")
    }

    fn expand_part(&mut self, part: &WordPart, quoted: bool) -> Vec<String> {
        match part {
            WordPart::Literal(value) | WordPart::SingleQuoted(value) | WordPart::Escaped(value) => {
                vec![value.clone()]
            }
            WordPart::DoubleQuoted(parts) => {
                let mut values = vec![String::new()];
                for part in parts {
                    values = append_expanded_values(values, self.expand_part(part, true));
                }
                vec![values.join(" ")]
            }
            WordPart::Parameter(parameter) => self.expand_parameter(parameter, quoted),
            WordPart::CommandSubstitution { body, .. } => {
                let output = self.exec_script(body);
                let mut stdout = output.stdout;
                while stdout.ends_with('\n') {
                    stdout.pop();
                }
                if quoted {
                    vec![stdout]
                } else {
                    vec![stdout.replace('\n', " ")]
                }
            }
            WordPart::Arithmetic(expression) => {
                vec![self.eval_arithmetic(&expression.source).to_string()]
            }
            WordPart::Brace(brace) => self.expand_brace(brace),
            WordPart::Tilde { user } => {
                if user.is_none() {
                    vec![self.lookup_parameter("HOME")]
                } else {
                    vec![format!("~{}", user.as_deref().unwrap_or_default())]
                }
            }
        }
    }

    fn expand_parameter(&mut self, parameter: &ParameterExpansion, quoted: bool) -> Vec<String> {
        if let Some(ParameterOperation::Length) = &parameter.operation {
            return vec![
                self.lookup_parameter(&parameter.parameter)
                    .len()
                    .to_string(),
            ];
        }

        let is_set = self.parameter_is_set(&parameter.parameter);
        let value = self.lookup_parameter(&parameter.parameter);
        match &parameter.operation {
            Some(ParameterOperation::DefaultValue { word, check_empty }) => {
                if !is_set || (*check_empty && value.is_empty()) {
                    vec![self.expand_word_to_string(word)]
                } else {
                    vec![value]
                }
            }
            Some(ParameterOperation::AssignDefault { word, check_empty }) => {
                if !is_set || (*check_empty && value.is_empty()) {
                    let default = self.expand_word_to_string(word);
                    self.state
                        .assign_var(parameter.parameter.clone(), default.clone());
                    vec![default]
                } else {
                    vec![value]
                }
            }
            Some(ParameterOperation::UseAlternative { word, check_empty }) => {
                if is_set && (!*check_empty || !value.is_empty()) {
                    vec![self.expand_word_to_string(word)]
                } else {
                    vec![String::new()]
                }
            }
            Some(ParameterOperation::Length) => unreachable!("handled above"),
            None => {
                if matches!(parameter.parameter.as_str(), "@" | "*") && !quoted {
                    self.state.positionals.clone()
                } else {
                    vec![value]
                }
            }
        }
    }

    fn lookup_parameter(&self, parameter: &str) -> String {
        match parameter {
            "?" => self.state.last_status.to_string(),
            "#" => self.state.positionals.len().to_string(),
            "@" | "*" => self.state.positionals.join(" "),
            "$" => "0".to_string(),
            _ if parameter
                .chars()
                .all(|character| character.is_ascii_digit()) =>
            {
                parameter
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| index.checked_sub(1))
                    .and_then(|index| self.state.positionals.get(index).cloned())
                    .unwrap_or_default()
            }
            _ => {
                if let Some((array, index)) = parse_array_reference(parameter) {
                    return self
                        .state
                        .arrays
                        .get(array)
                        .and_then(|values| match index {
                            ArrayIndex::Index(index) => values.get(index).cloned(),
                            ArrayIndex::All => Some(values.join(" ")),
                        })
                        .unwrap_or_default();
                }
                self.state
                    .lookup_var(parameter)
                    .unwrap_or_default()
                    .to_string()
            }
        }
    }

    fn parameter_is_set(&self, parameter: &str) -> bool {
        if matches!(parameter, "?" | "#" | "@" | "*" | "$") {
            return true;
        }
        if parse_array_reference(parameter).is_some() {
            return !self.lookup_parameter(parameter).is_empty();
        }
        self.state.lookup_var(parameter).is_some()
    }

    fn expand_brace(&mut self, brace: &BraceExpansion) -> Vec<String> {
        let mut values = Vec::new();
        for item in &brace.items {
            match item {
                BraceItem::Word(word) => values.push(self.expand_word_to_string(word)),
                BraceItem::Range { start, end, step } => {
                    values.extend(expand_range(start, end, *step));
                }
            }
        }
        values
    }

    fn expand_inline_text(&mut self, text: &str) -> String {
        match parse(text) {
            Ok(script) if script.statements.len() == 1 => text
                .strip_prefix('$')
                .map_or_else(|| text.to_string(), |name| self.lookup_parameter(name)),
            _ => text.to_string(),
        }
    }

    fn eval_arithmetic(&mut self, source: &str) -> i64 {
        ArithmeticEvaluator::new(source, &mut self.state).parse()
    }

    /// Drain any pending arithmetic error raised during expansion (e.g. division
    /// by zero) into a failing command result, matching bash which prints the
    /// diagnostic on stderr and returns exit status 1.
    fn take_arith_error(&mut self) -> Option<ExecOutput> {
        self.state.arith_error.take().map(|stderr| ExecOutput {
            stdout: String::new(),
            stderr,
            exit_code: 1,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ExpandedAssignment {
    Scalar {
        name: String,
        value: String,
        append: bool,
    },
    Array {
        name: String,
        values: Vec<String>,
    },
    ArrayElement {
        name: String,
        index: usize,
        value: String,
        append: bool,
    },
}

fn prepend_trace(mut output: ExecOutput, trace: Option<String>) -> ExecOutput {
    if let Some(mut trace) = trace {
        trace.push_str(&output.stderr);
        output.stderr = trace;
    }
    output
}

fn format_expanded_assignment(assignment: &ExpandedAssignment) -> Option<String> {
    match assignment {
        ExpandedAssignment::Scalar {
            name,
            value,
            append,
        } => Some(format!(
            "{name}{}{value}",
            if *append { "+=" } else { "=" },
            value = trace_quote_arg(value)
        )),
        ExpandedAssignment::Array { name, values } => Some(format!(
            "{name}=({})",
            values
                .iter()
                .map(|value| trace_quote_arg(value))
                .collect::<Vec<_>>()
                .join(" ")
        )),
        ExpandedAssignment::ArrayElement {
            name,
            index,
            value,
            append,
        } => Some(format!(
            "{name}[{index}]{}{value}",
            if *append { "+=" } else { "=" },
            value = trace_quote_arg(value)
        )),
    }
}

fn trace_quote_arg(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    if value.chars().all(|character| {
        character.is_ascii_alphanumeric()
            || matches!(character, '_' | '-' | '+' | '/' | '.' | ':' | '=')
    }) {
        value.to_string()
    } else {
        shell_quote_arg(value)
    }
}

fn expand_trace_prefix(raw: &str, state: &ShellState) -> String {
    let chars = raw.chars().collect::<Vec<_>>();
    let mut output = String::new();
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] != '$' {
            output.push(chars[index]);
            index += 1;
            continue;
        }

        if chars.get(index + 1) == Some(&'{')
            && let Some(end) = chars[index + 2..]
                .iter()
                .position(|character| *character == '}')
        {
            let name = chars[index + 2..index + 2 + end].iter().collect::<String>();
            output.push_str(&lookup_trace_parameter(&name, state));
            index += end + 3;
            continue;
        }

        if chars
            .get(index + 1)
            .is_some_and(|character| is_name_start(*character))
        {
            let mut end = index + 2;
            while chars
                .get(end)
                .is_some_and(|character| is_name_continue(*character))
            {
                end += 1;
            }
            let name = chars[index + 1..end].iter().collect::<String>();
            output.push_str(&lookup_trace_parameter(&name, state));
            index = end;
            continue;
        }

        output.push('$');
        index += 1;
    }
    output
}

fn lookup_trace_parameter(name: &str, state: &ShellState) -> String {
    if name == "LINENO" {
        return state.command_count.to_string();
    }
    state.lookup_var(name).unwrap_or_default().to_string()
}

fn append_expanded_values(prefixes: Vec<String>, suffixes: Vec<String>) -> Vec<String> {
    let mut output = Vec::new();
    for prefix in prefixes {
        for suffix in &suffixes {
            let mut value = prefix.clone();
            value.push_str(suffix);
            output.push(value);
        }
    }
    output
}

fn strip_leading_tabs_from_lines(content: &str) -> String {
    content
        .split_inclusive('\n')
        .map(|line| line.trim_start_matches('\t'))
        .collect()
}

fn find_double_right_paren(chars: &[char], start: usize) -> Option<usize> {
    let mut index = start;
    while index + 1 < chars.len() {
        if chars[index] == ')' && chars[index + 1] == ')' {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn find_matching_substitution_paren(chars: &[char], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote = None;
    let mut index = start;
    while index < chars.len() {
        let character = chars[index];
        if let Some(quote_char) = quote {
            if character == '\\' {
                index += 2;
                continue;
            }
            if character == quote_char {
                quote = None;
            }
            index += 1;
            continue;
        }

        match character {
            '\'' | '"' => quote = Some(character),
            '(' => depth += 1,
            ')' if depth == 0 => return Some(index),
            ')' => depth -= 1,
            _ => {}
        }
        index += 1;
    }
    None
}

fn write_or_discard(files: &mut ShellVirtualFileSystem, target: &str, content: &str, append: bool) {
    if target == "/dev/null" {
        return;
    }
    if append {
        files.append(target.to_string(), content);
    } else {
        files.write(target.to_string(), content.to_string());
    }
}

fn alias_command_source(alias: &str, args: &[String]) -> String {
    let mut source = alias.to_string();
    for arg in args {
        source.push(' ');
        source.push_str(&shell_quote(arg));
    }
    source
}

fn shell_quote(value: &str) -> String {
    if value.chars().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '/' | '.' | ':')
    }) {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

pub fn shell_join_args<I, S>(args: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .map(|arg| shell_quote_arg(arg.as_ref()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote_arg(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Splits an `alias name=value` argument into its name and value.
///
/// Mirrors the upstream alias builtin: an argument is treated as a definition
/// only when it contains an `=`, the name is everything before the first `=`,
/// and a single matching pair of surrounding quotes is stripped from the value
/// (word expansion usually removes these already, but the builtin strips any
/// that survive). Returns `None` when there is no `=`, which signals a lookup.
fn split_alias_assignment(arg: &str) -> Option<(String, String)> {
    let equals = arg.find('=')?;
    let name = arg[..equals].to_string();
    let mut value = arg[equals + 1..].to_string();
    let bytes = value.as_bytes();
    if value.len() >= 2
        && ((bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
            || (bytes[0] == b'"' && bytes[value.len() - 1] == b'"'))
    {
        value = value[1..value.len() - 1].to_string();
    }
    Some((name, value))
}

fn alias_help_text() -> String {
    let mut output = String::from("alias - define or display aliases\n\n");
    output.push_str("Usage: alias [name[=value] ...]\n");
    output.push_str("\nOptions:\n");
    output.push_str("      --help display this help and exit\n");
    output
}

fn unalias_help_text() -> String {
    let mut output = String::from("unalias - remove alias definitions\n\n");
    output.push_str("Usage: unalias name [name ...]\n");
    output.push_str("\nOptions:\n");
    output.push_str("  -a      remove all aliases\n");
    output.push_str("      --help display this help and exit\n");
    output
}

fn split_assignment_text(text: &str) -> Option<(String, String)> {
    let equals = text.find('=')?;
    let name = text[..equals].to_string();
    is_valid_name(&name).then(|| (name, text[equals + 1..].to_string()))
}

fn format_export_listing(env: &BTreeMap<String, String>) -> String {
    env.iter()
        .map(|(name, value)| format!("declare -x {name}=\"{}\"\n", quote_export_value(value)))
        .collect()
}

fn quote_export_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn parse_array_reference(parameter: &str) -> Option<(&str, ArrayIndex)> {
    let open = parameter.find('[')?;
    let close = parameter.strip_suffix(']')?;
    let name = &parameter[..open];
    if !is_valid_name(name) {
        return None;
    }
    let index = &close[open + 1..];
    if matches!(index, "@" | "*") {
        Some((name, ArrayIndex::All))
    } else {
        index
            .parse::<usize>()
            .ok()
            .map(|index| (name, ArrayIndex::Index(index)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArrayIndex {
    Index(usize),
    All,
}

fn expand_range(start: &str, end: &str, step: Option<i64>) -> Vec<String> {
    if let (Ok(start_num), Ok(end_num)) = (start.parse::<i64>(), end.parse::<i64>()) {
        let width = start.len().max(end.len());
        let mut values = Vec::new();
        let default_step = if start_num <= end_num { 1 } else { -1 };
        let step = step.unwrap_or(default_step);
        if step == 0 {
            return values;
        }
        let mut current = start_num;
        while (step > 0 && current <= end_num) || (step < 0 && current >= end_num) {
            if start.starts_with('0') || end.starts_with('0') {
                values.push(format!("{current:0width$}"));
            } else {
                values.push(current.to_string());
            }
            current += step;
        }
        return values;
    }

    let mut start_chars = start.chars();
    let mut end_chars = end.chars();
    if let (Some(start_char), None, Some(end_char), None) = (
        start_chars.next(),
        start_chars.next(),
        end_chars.next(),
        end_chars.next(),
    ) {
        let start_num = start_char as i64;
        let end_num = end_char as i64;
        let step = step.unwrap_or(if start_num <= end_num { 1 } else { -1 });
        let mut values = Vec::new();
        let mut current = start_num;
        while (step > 0 && current <= end_num) || (step < 0 && current >= end_num) {
            if let Some(character) = char::from_u32(current as u32) {
                values.push(character.to_string());
            }
            current += step;
        }
        return values;
    }

    vec![format!("{{{start}..{end}}}")]
}

fn pattern_matches(pattern: &str, value: &str) -> bool {
    let pattern = pattern.chars().collect::<Vec<_>>();
    let value = value.chars().collect::<Vec<_>>();
    glob_match(&pattern, 0, &value, 0)
}

fn glob_match(pattern: &[char], pattern_index: usize, value: &[char], value_index: usize) -> bool {
    if pattern_index == pattern.len() {
        return value_index == value.len();
    }

    match pattern[pattern_index] {
        '*' => {
            let next_pattern = pattern_index + 1;
            if next_pattern == pattern.len() {
                return true;
            }
            (value_index..=value.len())
                .any(|next_value| glob_match(pattern, next_pattern, value, next_value))
        }
        '?' => {
            value_index < value.len()
                && glob_match(pattern, pattern_index + 1, value, value_index + 1)
        }
        '[' => {
            let Some((matched, next_pattern)) =
                match_character_class(pattern, pattern_index, value.get(value_index).copied())
            else {
                return value_index < value.len()
                    && pattern[pattern_index] == value[value_index]
                    && glob_match(pattern, pattern_index + 1, value, value_index + 1);
            };
            matched && glob_match(pattern, next_pattern, value, value_index + 1)
        }
        literal => {
            value_index < value.len()
                && literal == value[value_index]
                && glob_match(pattern, pattern_index + 1, value, value_index + 1)
        }
    }
}

fn match_character_class(
    pattern: &[char],
    start: usize,
    value: Option<char>,
) -> Option<(bool, usize)> {
    let mut end = start + 1;
    while end < pattern.len() && pattern[end] != ']' {
        end += 1;
    }
    if end >= pattern.len() || end == start + 1 {
        return None;
    }

    let value = value?;
    let mut index = start + 1;
    let negated = matches!(pattern.get(index), Some('!' | '^'));
    if negated {
        index += 1;
    }

    let mut matched = false;
    while index < end {
        let current = pattern[index];
        if index + 2 < end && pattern[index + 1] == '-' {
            let range_end = pattern[index + 2];
            matched |= current <= value && value <= range_end;
            index += 3;
        } else {
            matched |= current == value;
            index += 1;
        }
    }

    Some((if negated { !matched } else { matched }, end + 1))
}

fn shell_words(source: &str) -> Vec<String> {
    Lexer::new(source)
        .tokenize()
        .map(|tokens| {
            tokens
                .into_iter()
                .filter_map(|token| match token.kind {
                    TokenKind::Word(word) => Some(word.plain_text().unwrap_or_default()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ArithToken {
    Number(i64),
    Ident(String),
    Op(String),
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    Question,
    Colon,
    Comma,
    End,
}

/// An assignable target within an arithmetic expression.
#[derive(Debug, Clone)]
enum ArithLValue {
    Scalar(String),
    Element { name: String, index: usize },
}

struct ArithmeticEvaluator<'a> {
    tokens: Vec<ArithToken>,
    pos: usize,
    state: &'a mut ShellState,
    /// Guard against runaway recursion when resolving variable names that
    /// refer to themselves (e.g. `a=a`).
    depth: usize,
}

impl<'a> ArithmeticEvaluator<'a> {
    fn new(source: &str, state: &'a mut ShellState) -> Self {
        Self {
            tokens: tokenize_arithmetic(source),
            pos: 0,
            state,
            depth: 0,
        }
    }

    fn parse(&mut self) -> i64 {
        // The top level is a comma sequence, returning the last value.
        let mut value = self.parse_assignment();
        while matches!(self.current(), ArithToken::Comma) {
            self.advance();
            value = self.parse_assignment();
        }
        value
    }

    /// Resolve a Bash variable reference to an integer. Bash recursively
    /// re-evaluates the variable's string value as an arithmetic expression
    /// (so `a=5; b=a` makes `$((b))` yield 5, and `e='1+2'` makes
    /// `$((e + 3))` yield 6).
    fn resolve_name(&mut self, name: &str) -> i64 {
        if self.depth > 64 {
            return 0;
        }
        // A positional parameter (`$1`, `$2`, ...) referenced inside arithmetic
        // resolves to its value. `$0` and out-of-range positions are 0.
        if !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_digit()) {
            let raw = match name.parse::<usize>() {
                Ok(0) | Err(_) => return 0,
                Ok(index) => match self.state.positionals.get(index - 1) {
                    Some(value) => value.clone(),
                    None => return 0,
                },
            };
            if raw.trim().is_empty() {
                return 0;
            }
            if let Some(value) = parse_arith_literal(raw.trim()) {
                return value;
            }
            let mut nested = ArithmeticEvaluator {
                tokens: tokenize_arithmetic(&raw),
                pos: 0,
                state: self.state,
                depth: self.depth + 1,
            };
            return nested.parse();
        }
        let Some(raw) = self.state.lookup_var(name).map(str::to_string) else {
            return 0;
        };
        if raw.trim().is_empty() {
            return 0;
        }
        if let Some(value) = parse_arith_literal(raw.trim()) {
            return value;
        }
        // Re-evaluate the stored string as an arithmetic expression.
        let mut nested = ArithmeticEvaluator {
            tokens: tokenize_arithmetic(&raw),
            pos: 0,
            state: self.state,
            depth: self.depth + 1,
        };
        nested.parse()
    }

    /// Lowest-precedence non-comma level: assignment (right-associative).
    fn parse_assignment(&mut self) -> i64 {
        let start = self.pos;
        if let Some(lvalue) = self.try_parse_lvalue() {
            if let ArithToken::Op(operator) = self.current().clone() {
                if is_assignment_op(&operator) {
                    self.advance();
                    let rhs = self.parse_assignment();
                    let current = self.read_lvalue(&lvalue);
                    let result = if operator == "=" {
                        rhs
                    } else {
                        self.apply_binary(&operator[..operator.len() - 1], current, rhs)
                    };
                    self.write_lvalue(&lvalue, result);
                    return result;
                }
            }
            // Not an assignment; rewind and fall through to the ternary parser.
            self.pos = start;
        }
        self.parse_ternary()
    }

    fn parse_ternary(&mut self) -> i64 {
        let condition = self.parse_binary(1);
        if matches!(self.current(), ArithToken::Question) {
            self.advance();
            let when_true = self.parse_assignment();
            if matches!(self.current(), ArithToken::Colon) {
                self.advance();
            }
            let when_false = self.parse_assignment();
            if condition != 0 {
                when_true
            } else {
                when_false
            }
        } else {
            condition
        }
    }

    fn parse_binary(&mut self, min_prec: u8) -> i64 {
        let mut left = self.parse_prefix();
        while let ArithToken::Op(operator) = self.current().clone() {
            let Some((precedence, right_assoc)) = arith_precedence(&operator) else {
                break;
            };
            if precedence < min_prec {
                break;
            }
            // Short-circuit logical operators must not evaluate (and must not
            // run side effects in) the right-hand side when the result is
            // already determined.
            if operator == "&&" && left == 0 {
                self.advance();
                self.skip_binary(precedence + 1);
                left = 0;
                continue;
            }
            if operator == "||" && left != 0 {
                self.advance();
                self.skip_binary(precedence + 1);
                left = 1;
                continue;
            }
            self.advance();
            let next_min = if right_assoc {
                precedence
            } else {
                precedence + 1
            };
            let right = self.parse_binary(next_min);
            left = match operator.as_str() {
                "&&" => i64::from(left != 0 && right != 0),
                "||" => i64::from(left != 0 || right != 0),
                _ => self.apply_binary(&operator, left, right),
            };
        }
        left
    }

    /// Apply a binary arithmetic operator, recording an arithmetic error on the
    /// shell state for division/modulo by zero and negative exponents. Mirrors
    /// bash, which aborts the expression and prints a diagnostic in these cases.
    fn apply_binary(&mut self, operator: &str, left: i64, right: i64) -> i64 {
        match operator {
            "/" | "%" if right == 0 => {
                self.set_arith_error("division by 0");
                0
            }
            "**" if right < 0 => {
                self.set_arith_error("exponent less than 0");
                0
            }
            _ => eval_arith_binary(operator, left, right),
        }
    }

    fn set_arith_error(&mut self, message: &str) {
        if self.state.arith_error.is_none() {
            self.state.arith_error = Some(format!("bash: {message}\n"));
        }
    }

    /// Consume (without producing observable side effects) the right operand of
    /// a short-circuited logical operator.
    fn skip_binary(&mut self, min_prec: u8) {
        let saved = self.state.clone();
        self.parse_binary(min_prec);
        *self.state = saved;
    }

    fn parse_prefix(&mut self) -> i64 {
        match self.current().clone() {
            ArithToken::Op(operator) if matches!(operator.as_str(), "+" | "-" | "!" | "~") => {
                self.advance();
                let value = self.parse_prefix();
                match operator.as_str() {
                    "+" => value,
                    "-" => -value,
                    "!" => i64::from(value == 0),
                    "~" => !value,
                    _ => value,
                }
            }
            ArithToken::Op(operator) if matches!(operator.as_str(), "++" | "--") => {
                self.advance();
                let Some(lvalue) = self.try_parse_lvalue() else {
                    return 0;
                };
                let current = self.read_lvalue(&lvalue);
                let updated = if operator == "++" {
                    current + 1
                } else {
                    current - 1
                };
                self.write_lvalue(&lvalue, updated);
                updated
            }
            ArithToken::LeftParen => {
                self.advance();
                let value = self.parse();
                if matches!(self.current(), ArithToken::RightParen) {
                    self.advance();
                }
                value
            }
            ArithToken::Number(value) => {
                self.advance();
                value
            }
            ArithToken::Ident(_) => {
                let lvalue = self
                    .try_parse_lvalue()
                    .expect("identifier token yields an lvalue");
                // Post-increment / post-decrement.
                if let ArithToken::Op(operator) = self.current().clone() {
                    if matches!(operator.as_str(), "++" | "--") {
                        self.advance();
                        let current = self.read_lvalue(&lvalue);
                        let updated = if operator == "++" {
                            current + 1
                        } else {
                            current - 1
                        };
                        self.write_lvalue(&lvalue, updated);
                        return current;
                    }
                }
                self.read_lvalue(&lvalue)
            }
            _ => {
                self.advance();
                0
            }
        }
    }

    /// Parse an assignable target (a bare name or `name[index]`). Returns `None`
    /// and consumes nothing extra if the next token is not an identifier.
    fn try_parse_lvalue(&mut self) -> Option<ArithLValue> {
        let ArithToken::Ident(name) = self.current().clone() else {
            return None;
        };
        self.advance();
        if matches!(self.current(), ArithToken::LeftBracket) {
            self.advance();
            let index = self.parse();
            if matches!(self.current(), ArithToken::RightBracket) {
                self.advance();
            }
            let index = usize::try_from(index).unwrap_or(0);
            Some(ArithLValue::Element { name, index })
        } else {
            Some(ArithLValue::Scalar(name))
        }
    }

    fn read_lvalue(&mut self, lvalue: &ArithLValue) -> i64 {
        match lvalue {
            ArithLValue::Scalar(name) => self.resolve_name(name),
            ArithLValue::Element { name, index } => self
                .state
                .arrays
                .get(name)
                .and_then(|values| values.get(*index))
                .and_then(|value| parse_arith_literal(value.trim()))
                .unwrap_or(0),
        }
    }

    fn write_lvalue(&mut self, lvalue: &ArithLValue, value: i64) {
        match lvalue {
            ArithLValue::Scalar(name) => {
                self.state.assign_var(name.clone(), value.to_string());
            }
            ArithLValue::Element { name, index } => {
                let values = self.state.arrays.entry(name.clone()).or_default();
                if values.len() <= *index {
                    values.resize(*index + 1, String::new());
                }
                values[*index] = value.to_string();
            }
        }
    }

    fn current(&self) -> &ArithToken {
        self.tokens.get(self.pos).unwrap_or(&ArithToken::End)
    }

    fn advance(&mut self) -> ArithToken {
        let token = self.current().clone();
        if !matches!(token, ArithToken::End) {
            self.pos += 1;
        }
        token
    }
}

fn is_assignment_op(operator: &str) -> bool {
    matches!(
        operator,
        "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "<<=" | ">>=" | "&=" | "|=" | "^="
    )
}

/// Parse a Bash arithmetic integer literal: decimal, `0x` hex, leading-zero
/// octal, or `base#digits` notation. Returns `None` for non-literal strings so
/// the caller can fall back to expression evaluation.
fn parse_arith_literal(text: &str) -> Option<i64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let (negative, body) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let magnitude = parse_unsigned_arith_literal(body)?;
    Some(if negative { -magnitude } else { magnitude })
}

fn parse_unsigned_arith_literal(body: &str) -> Option<i64> {
    if let Some(hex) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
        return i64::from_str_radix(hex, 16).ok();
    }
    if let Some((base, digits)) = body.split_once('#') {
        let radix: u32 = base.parse().ok()?;
        if !(2..=36).contains(&radix) {
            return None;
        }
        return i64::from_str_radix(digits, radix).ok();
    }
    if body.len() > 1 && body.starts_with('0') && body.bytes().all(|byte| byte.is_ascii_digit()) {
        return i64::from_str_radix(body, 8).ok();
    }
    body.parse::<i64>().ok()
}

fn tokenize_arithmetic(source: &str) -> Vec<ArithToken> {
    let chars = source.chars().collect::<Vec<_>>();
    let mut pos = 0usize;
    let mut tokens = Vec::new();
    while let Some(character) = chars.get(pos).copied() {
        if character.is_whitespace() {
            pos += 1;
            continue;
        }
        if character.is_ascii_digit() {
            let start = pos;
            // Consume the longest run of characters that can form a numeric
            // literal: decimal/hex/octal digits, `x`/`X` (hex prefix), `#`
            // (base#digits notation), and alphanumerics for the digits of a
            // base# literal.
            while chars
                .get(pos)
                .is_some_and(|character| character.is_ascii_alphanumeric() || *character == '#')
            {
                pos += 1;
            }
            let text = chars[start..pos].iter().collect::<String>();
            tokens.push(ArithToken::Number(parse_arith_literal(&text).unwrap_or(0)));
            continue;
        }
        if character == '$' {
            pos += 1;
            let start = pos;
            while chars
                .get(pos)
                .is_some_and(|character| is_name_continue(*character))
            {
                pos += 1;
            }
            tokens.push(ArithToken::Ident(chars[start..pos].iter().collect()));
            continue;
        }
        if is_name_start(character) {
            let start = pos;
            while chars
                .get(pos)
                .is_some_and(|character| is_name_continue(*character))
            {
                pos += 1;
            }
            tokens.push(ArithToken::Ident(chars[start..pos].iter().collect()));
            continue;
        }
        if character == '(' {
            tokens.push(ArithToken::LeftParen);
            pos += 1;
            continue;
        }
        if character == ')' {
            tokens.push(ArithToken::RightParen);
            pos += 1;
            continue;
        }
        if character == '[' {
            tokens.push(ArithToken::LeftBracket);
            pos += 1;
            continue;
        }
        if character == ']' {
            tokens.push(ArithToken::RightBracket);
            pos += 1;
            continue;
        }
        if character == '?' {
            tokens.push(ArithToken::Question);
            pos += 1;
            continue;
        }
        if character == ':' {
            tokens.push(ArithToken::Colon);
            pos += 1;
            continue;
        }
        if character == ',' {
            tokens.push(ArithToken::Comma);
            pos += 1;
            continue;
        }
        let mut matched = false;
        // Longest-match first so compound assignment / increment operators win
        // over their single-character prefixes.
        for operator in [
            "<<=", ">>=", "**", "++", "--", "<<", ">>", "<=", ">=", "==", "!=", "&&", "||", "+=",
            "-=", "*=", "/=", "%=", "&=", "|=", "^=", "+", "-", "*", "/", "%", "<", ">", "&", "|",
            "^", "!", "~", "=",
        ] {
            if chars[pos..].iter().take(operator.len()).collect::<String>() == operator {
                tokens.push(ArithToken::Op(operator.to_string()));
                pos += operator.len();
                matched = true;
                break;
            }
        }
        if !matched {
            pos += 1;
        }
    }
    tokens.push(ArithToken::End);
    tokens
}

fn arith_precedence(operator: &str) -> Option<(u8, bool)> {
    let value = match operator {
        "||" => (1, false),
        "&&" => (2, false),
        "|" => (3, false),
        "^" => (4, false),
        "&" => (5, false),
        "==" | "!=" => (6, false),
        "<" | "<=" | ">" | ">=" => (7, false),
        "<<" | ">>" => (8, false),
        "+" | "-" => (9, false),
        "*" | "/" | "%" => (10, false),
        "**" => (11, true),
        _ => return None,
    };
    Some(value)
}

/// Split the source of a C-style `for (( init; cond; update ))` header into its
/// three semicolon-separated clauses. Splitting is done at depth-0 semicolons so
/// nested parentheses (e.g. function-call-like grouping) are preserved.
fn split_for_arith_clauses(source: &str) -> (String, String, String) {
    let mut clauses: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    for ch in source.chars() {
        match ch {
            '(' => {
                depth += 1;
                current.push(ch);
            }
            ')' => {
                depth -= 1;
                current.push(ch);
            }
            ';' if depth == 0 => {
                clauses.push(current.trim().to_string());
                current = String::new();
            }
            _ => current.push(ch),
        }
    }
    clauses.push(current.trim().to_string());
    let init = clauses.first().cloned().unwrap_or_default();
    let condition = clauses.get(1).cloned().unwrap_or_default();
    let update = clauses.get(2).cloned().unwrap_or_default();
    (init, condition, update)
}

/// Word-split a value into fields using the current `IFS`. When `IFS` is unset
/// the default is space/tab/newline. IFS whitespace runs collapse and are
/// trimmed from the ends; each non-whitespace IFS character delimits one field.
/// Mirrors bash field splitting for the behaviors exercised by the interpreter
/// conformance suite.
fn split_on_ifs(value: &str, ifs: Option<&str>) -> Vec<String> {
    let ifs = ifs.unwrap_or(" \t\n");
    if ifs.is_empty() {
        // An empty IFS performs no splitting.
        return if value.is_empty() {
            Vec::new()
        } else {
            vec![value.to_string()]
        };
    }
    let ws: Vec<char> = ifs
        .chars()
        .filter(|c| matches!(c, ' ' | '\t' | '\n'))
        .collect();
    let non_ws: Vec<char> = ifs
        .chars()
        .filter(|c| !matches!(c, ' ' | '\t' | '\n'))
        .collect();
    let is_ws = |c: char| ws.contains(&c);
    let is_sep = |c: char| non_ws.contains(&c);

    let mut fields: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut have_field = false;
    let mut chars = value.chars().peekable();
    // Skip leading IFS whitespace.
    while let Some(&c) = chars.peek() {
        if is_ws(c) {
            chars.next();
        } else {
            break;
        }
    }
    while let Some(c) = chars.next() {
        if is_ws(c) {
            // Collapse a run of IFS whitespace; it terminates the current field.
            fields.push(std::mem::take(&mut current));
            have_field = false;
            while let Some(&n) = chars.peek() {
                if is_ws(n) {
                    chars.next();
                } else {
                    break;
                }
            }
            // Trailing whitespace at end of input produces no extra field.
            if chars.peek().is_none() {
                return fields;
            }
        } else if is_sep(c) {
            // A non-whitespace IFS delimiter always closes a field, even empty.
            fields.push(std::mem::take(&mut current));
            have_field = false;
            // Absorb surrounding IFS whitespace around the delimiter.
            while let Some(&n) = chars.peek() {
                if is_ws(n) {
                    chars.next();
                } else {
                    break;
                }
            }
        } else {
            current.push(c);
            have_field = true;
        }
    }
    if have_field || !current.is_empty() {
        fields.push(current);
    }
    fields
}

fn eval_arith_binary(operator: &str, left: i64, right: i64) -> i64 {
    match operator {
        "+" => left + right,
        "-" => left - right,
        "*" => left * right,
        "/" => {
            if right == 0 {
                0
            } else {
                left / right
            }
        }
        "%" => {
            if right == 0 {
                0
            } else {
                left % right
            }
        }
        "**" => left.saturating_pow(right.max(0) as u32),
        "<<" => left << right.max(0),
        ">>" => left >> right.max(0),
        "<" => i64::from(left < right),
        "<=" => i64::from(left <= right),
        ">" => i64::from(left > right),
        ">=" => i64::from(left >= right),
        "==" => i64::from(left == right),
        "!=" => i64::from(left != right),
        "&" => left & right,
        "|" => left | right,
        "^" => left ^ right,
        "&&" => i64::from(left != 0 && right != 0),
        "||" => i64::from(left != 0 || right != 0),
        _ => 0,
    }
}

fn is_special_parameter(character: char) -> bool {
    matches!(
        character,
        '?' | '#' | '@' | '*' | '$' | '!' | '-' | '0'..='9'
    )
}

fn is_name_start(character: char) -> bool {
    character.is_ascii_alphabetic() || character == '_'
}

fn is_name_continue(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(is_name_start) && chars.all(is_name_continue)
}

fn parse_i64(value: &str) -> i64 {
    value.parse::<i64>().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeCommands {
        invocations: Vec<CommandInvocation>,
    }

    impl CommandDispatcher for FakeCommands {
        fn dispatch(
            &mut self,
            invocation: CommandInvocation,
            files: &mut ShellVirtualFileSystem,
        ) -> CommandResult {
            self.invocations.push(invocation.clone());
            match invocation.name.as_str() {
                "echo" => fake_echo(&invocation.args),
                "printf" => fake_printf(&invocation.args),
                "cat" => fake_cat(&invocation, files),
                "grep" => fake_grep(&invocation, files),
                "sort" => fake_sort(&invocation),
                "[" | "test" => fake_test(&invocation.args),
                "wc" if invocation.args.first().map(String::as_str) == Some("-l") => {
                    let count = invocation.stdin.lines().count();
                    CommandResult::success(format!("{count}\n"))
                }
                "err" => CommandResult {
                    stdout: invocation
                        .args
                        .first()
                        .map_or_else(String::new, |value| format!("{value}\n")),
                    stderr: invocation
                        .args
                        .get(1)
                        .map_or_else(|| "err\n".to_string(), |value| format!("{value}\n")),
                    exit_code: invocation
                        .args
                        .get(2)
                        .and_then(|arg| arg.parse::<i32>().ok())
                        .unwrap_or(0),
                },
                "status" => CommandResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: invocation
                        .args
                        .first()
                        .and_then(|arg| arg.parse::<i32>().ok())
                        .unwrap_or(0),
                },
                _ => {
                    CommandResult::failure(format!("{}: command not found\n", invocation.name), 127)
                }
            }
        }
    }

    fn fake_echo(args: &[String]) -> CommandResult {
        let mut interpret_escapes = false;
        let mut args = args;
        if args.first().map(String::as_str) == Some("-e") {
            interpret_escapes = true;
            args = &args[1..];
        }
        let mut stdout = args.join(" ");
        if interpret_escapes {
            stdout = stdout
                .replace("\\n", "\n")
                .replace("\\t", "\t")
                .replace("\\\\", "\\");
        }
        stdout.push('\n');
        CommandResult::success(stdout)
    }

    fn fake_printf(args: &[String]) -> CommandResult {
        if args.first().map(String::as_str) == Some("%s|") {
            return CommandResult::success(
                args.iter()
                    .skip(1)
                    .map(|arg| format!("{arg}|"))
                    .collect::<String>(),
            );
        }
        CommandResult::success(
            args.join("")
                .replace("\\n", "\n")
                .replace("\\t", "\t")
                .replace("\\\\", "\\"),
        )
    }

    fn fake_cat(invocation: &CommandInvocation, files: &ShellVirtualFileSystem) -> CommandResult {
        if invocation.args.is_empty() {
            return CommandResult::success(invocation.stdin.clone());
        }
        let mut stdout = String::new();
        for path in &invocation.args {
            let Some(content) = files.read_to_string(path) else {
                return CommandResult::failure(format!("cat: {path}: No such file\n"), 1);
            };
            stdout.push_str(content);
        }
        CommandResult::success(stdout)
    }

    fn fake_grep(invocation: &CommandInvocation, files: &ShellVirtualFileSystem) -> CommandResult {
        let mut quiet = false;
        let mut args = invocation.args.iter();
        let first = args.next();
        let pattern = if first.map(String::as_str) == Some("-q") {
            quiet = true;
            args.next()
        } else {
            first
        };
        let Some(pattern) = pattern else {
            return CommandResult::failure("grep: missing pattern\n", 2);
        };
        let mut haystack = invocation.stdin.clone();
        for path in args {
            if let Some(content) = files.read_to_string(path) {
                haystack.push_str(content);
            }
        }
        if haystack.contains(pattern) {
            if quiet {
                CommandResult::success("")
            } else {
                let matches = haystack
                    .lines()
                    .filter(|line| line.contains(pattern))
                    .map(|line| format!("{line}\n"))
                    .collect::<String>();
                CommandResult::success(matches)
            }
        } else {
            CommandResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 1,
            }
        }
    }

    fn fake_test(args: &[String]) -> CommandResult {
        let args = args.strip_suffix(&["]".to_string()]).unwrap_or(args);
        let matched = match args {
            [unary, value] if unary == "-n" || unary == "-z" => {
                let nonempty = !value.is_empty();
                if unary == "-n" { nonempty } else { !nonempty }
            }
            [left, operator, right] => match operator.as_str() {
                "=" | "==" => left == right,
                "!=" => left != right,
                "-lt" | "-le" | "-gt" | "-ge" | "-eq" | "-ne" => {
                    let left = left.parse::<i64>().unwrap_or(0);
                    let right = right.parse::<i64>().unwrap_or(0);
                    match operator.as_str() {
                        "-lt" => left < right,
                        "-le" => left <= right,
                        "-gt" => left > right,
                        "-ge" => left >= right,
                        "-eq" => left == right,
                        "-ne" => left != right,
                        _ => unreachable!(),
                    }
                }
                _ => false,
            },
            [value] => !value.is_empty(),
            _ => false,
        };
        CommandResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: i32::from(!matched),
        }
    }

    fn fake_sort(invocation: &CommandInvocation) -> CommandResult {
        let mut lines = invocation
            .stdin
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>();
        lines.sort();
        let mut stdout = lines.join("\n");
        if !stdout.is_empty() {
            stdout.push('\n');
        }
        CommandResult::success(stdout)
    }

    fn shell() -> Interpreter<FakeCommands> {
        Interpreter::new(FakeCommands::default())
    }

    /// Mirrors the `TeePlugin exec` / `TeePlugin semantics preservation`
    /// describe blocks in upstream
    /// `packages/just-bash/src/transform/plugins/tee-plugin.test.ts`. Those
    /// suites run each script through `Bash().exec(...)` and assert that the
    /// TeePlugin-wrapped run yields byte-identical stdout/stderr/exitCode to a
    /// plain run. The TeePlugin wrapper exists only to mirror pipeline output to
    /// log files, so the contract being verified is purely that the interpreter
    /// produces the documented stdout/stderr/exit code for each construct. This
    /// Rust test asserts that exact interpreter output for the mapped rows; it
    /// fails if any control-flow, pipeline, `$?`, negation, arithmetic, or
    /// case-statement semantics regress.
    #[test]
    fn just_bash_transform_plugins_tee_semantics_match_upstream() {
        struct Case {
            script: &'static str,
            stdout: &'static str,
            stderr: &'static str,
            exit_code: i32,
        }

        let cases = [
            // tee-plugin.test.ts:329 simple success: echo hello
            Case {
                script: "echo hello",
                stdout: "hello\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:333 simple failure: false
            Case {
                script: "false",
                stdout: "",
                stderr: "",
                exit_code: 1,
            },
            // tee-plugin.test.ts:343 pipeline failure (last cmd): echo hello | grep nomatch
            Case {
                script: "echo hello | grep nomatch",
                stdout: "",
                stderr: "",
                exit_code: 1,
            },
            // tee-plugin.test.ts:347 multiple statements: echo a; echo b; echo c
            Case {
                script: "echo a; echo b; echo c",
                stdout: "a\nb\nc\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:351 logical AND: echo first && echo second
            Case {
                script: "echo first && echo second",
                stdout: "first\nsecond\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:355 logical OR: false || echo fallback
            Case {
                script: "false || echo fallback",
                stdout: "fallback\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:359 AND with failure: false && echo unreachable
            Case {
                script: "false && echo unreachable",
                stdout: "",
                stderr: "",
                exit_code: 1,
            },
            // tee-plugin.test.ts:363 variable assignment then use: VAR=hello; echo $VAR
            Case {
                script: "VAR=hello; echo $VAR",
                stdout: "hello\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:367 compound commands: if true; then echo yes; fi
            Case {
                script: "if true; then echo yes; fi",
                stdout: "yes\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:371 for loop: for i in a b c; do echo $i; done
            Case {
                script: "for i in a b c; do echo $i; done",
                stdout: "a\nb\nc\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:375 subshell: (echo sub)
            Case {
                script: "(echo sub)",
                stdout: "sub\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:379 group: { echo grp; }
            Case {
                script: "{ echo grp; }",
                stdout: "grp\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:389 exit code via $?: false; echo $?
            Case {
                script: "false; echo $?",
                stdout: "1\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:393 multiline output: printf 'a\nb\nc\n'
            Case {
                script: "printf 'a\\nb\\nc\\n'",
                stdout: "a\nb\nc\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:397 no-output command: true
            Case {
                script: "true",
                stdout: "",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:427 chained exit codes with $? across statements
            Case {
                script: "true; A=$?; false; B=$?; echo $A $B",
                stdout: "0 1\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:431 for loop piping into command
            Case {
                script: "for i in 3 1 2; do echo $i; done | sort",
                stdout: "1\n2\n3\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:445 arithmetic and conditionals mixed with commands
            Case {
                script: "x=5; y=3; echo $(( x + y )); (( x > y )) && echo bigger",
                stdout: "8\nbigger\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:463 nested subshells with variable isolation
            Case {
                script: "X=outer; (X=inner; echo $X); echo $X",
                stdout: "inner\nouter\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:467 case statement with fallthrough patterns
            Case {
                script: "for f in foo.txt bar.sh baz.py; do case \"$f\" in *.txt) echo text;; *.sh) echo shell;; *) echo other;; esac; done",
                stdout: "text\nshell\nother\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:503 pipeline exit code propagation through $?
            Case {
                script: "echo hello | grep hello; A=$?; echo hello | grep nope; B=$?; echo $A $B",
                stdout: "hello\n0 1\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:541 multi-pipeline with mixed success/failure and $?
            Case {
                script: "true; echo $?; false; echo $?; true; echo $?",
                stdout: "0\n1\n0\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:557 variable in loop body used after loop
            Case {
                script: "total=0; for n in 1 2 3 4 5; do total=$(( total + n )); done; echo $total",
                stdout: "15\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:563 nested if/else with commands
            Case {
                script: "X=42\nif [ $X -gt 100 ]; then\n  echo big\nelif [ $X -gt 10 ]; then\n  echo medium\nelse\n  echo small\nfi",
                stdout: "medium\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:618 subshell exit code does not leak
            Case {
                script: "(exit 42); echo $?",
                stdout: "42\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:637 exit code from last command in multi-statement script
            Case {
                script: "echo a; echo b; echo c; false",
                stdout: "a\nb\nc\n",
                stderr: "",
                exit_code: 1,
            },
            // tee-plugin.test.ts:645 empty pipeline commands
            Case {
                script: "true | true | true; echo $?",
                stdout: "0\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:685 negated pipeline: ! false | true
            Case {
                script: "! false | true; echo $?",
                stdout: "1\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:689 negated pipeline: ! true | false
            Case {
                script: "! true | false; echo $?",
                stdout: "0\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:703 $? after wrapped pipeline
            Case {
                script: "echo hello | grep nope; echo exit:$?",
                stdout: "exit:1\n",
                stderr: "",
                exit_code: 0,
            },
        ];

        for case in cases {
            let mut shell = Interpreter::new(FakeCommands::default());
            let result = shell.exec(case.script);
            assert_eq!(result.stdout, case.stdout, "stdout for: {}", case.script);
            assert_eq!(result.stderr, case.stderr, "stderr for: {}", case.script);
            assert_eq!(
                result.exit_code, case.exit_code,
                "exit code for: {}",
                case.script
            );
        }
    }

    /// Mirrors portable `packages/just-bash/src/syntax/parse-errors.test.ts`
    /// rows 1:1 through the Rust parser/interpreter. Each block asserts the
    /// upstream `it(...)` expectation on `Bash().exec(...)`: a syntax/parse error
    /// (non-zero exit, "syntax error" stderr), an accepted construct, or the
    /// "result is defined" graceful-handling rows. Rows requiring filesystem
    /// command families (auto-create-dir redirect L147, path-not-found L172,
    /// cat L179) and the runtime invalid-identifier row (L64) stay pending.
    #[test]
    fn jbpi_syntax_parse_errors_match_upstream() {
        // L6 unclosed if -> non-zero exit, "syntax error".
        let r = shell().exec("if true; then echo hello");
        assert_ne!(r.exit_code, 0, "unclosed if");
        assert!(
            r.stderr.contains("syntax error"),
            "unclosed if: {:?}",
            r.stderr
        );

        // L13 missing then -> non-zero exit.
        let r = shell().exec("if true; echo hello; fi");
        assert_ne!(r.exit_code, 0, "missing then");

        // L19 elif with valid condition selects the matching branch.
        let r = shell().exec("if false; then echo a; elif true; then echo b; fi");
        assert_eq!(r.stdout, "b\n", "elif condition");

        // L29 else without if -> syntax error (exit 2).
        let r = shell().exec("else echo hello; fi");
        assert_eq!(r.exit_code, 2, "else without if");

        // L35 fi without if -> syntax error (exit 2).
        let r = shell().exec("fi");
        assert_eq!(r.exit_code, 2, "fi without if");

        // L43 for missing `in` -> syntax error (exit 2).
        let r = shell().exec("for x a b c; do echo $x; done");
        assert_eq!(r.exit_code, 2, "for missing in");
        assert!(
            r.stderr.contains("syntax error"),
            "for missing in: {:?}",
            r.stderr
        );

        // L50 for missing `do` -> syntax error (exit 2).
        let r = shell().exec("for x in a b c; echo $x; done");
        assert_eq!(r.exit_code, 2, "for missing do");
        assert!(
            r.stderr.contains("syntax error"),
            "for missing do: {:?}",
            r.stderr
        );

        // L57 for missing `done` -> syntax error (exit 2).
        let r = shell().exec("for x in a b c; do echo $x");
        assert_eq!(r.exit_code, 2, "for missing done");
        assert!(
            r.stderr.contains("syntax error"),
            "for missing done: {:?}",
            r.stderr
        );

        // L75 while missing `do` -> syntax error (exit 2).
        let r = shell().exec("while true; echo loop; done");
        assert_eq!(r.exit_code, 2, "while missing do");
        assert!(
            r.stderr.contains("syntax error"),
            "while missing do: {:?}",
            r.stderr
        );

        // L82 while missing `done` -> syntax error (exit 2).
        let r = shell().exec("while true; do echo loop");
        assert_eq!(r.exit_code, 2, "while missing done");
        assert!(
            r.stderr.contains("syntax error"),
            "while missing done: {:?}",
            r.stderr
        );

        // L89 `while;` -> syntax error (exit 2).
        let r = shell().exec("while; do echo loop; done");
        assert_eq!(r.exit_code, 2, "while semicolon");
        assert!(
            r.stderr.contains("syntax error"),
            "while semicolon: {:?}",
            r.stderr
        );

        // L99 until missing `do` -> syntax error (exit 2).
        let r = shell().exec("until true; echo loop; done");
        assert_eq!(r.exit_code, 2, "until missing do");
        assert!(
            r.stderr.contains("syntax error"),
            "until missing do: {:?}",
            r.stderr
        );

        // L106 until missing `done` -> syntax error (exit 2).
        let r = shell().exec("until true; do echo loop");
        assert_eq!(r.exit_code, 2, "until missing done");
        assert!(
            r.stderr.contains("syntax error"),
            "until missing done: {:?}",
            r.stderr
        );

        // L115 function name starting with a digit is accepted (exit 0).
        let r = shell().exec("123func() { echo hello; }");
        assert_eq!(r.exit_code, 0, "numeric function name");

        // L122 unclosed function body -> syntax error (exit 2).
        let r = shell().exec("myfunc() { echo hello");
        assert_eq!(r.exit_code, 2, "unclosed function body");
        assert!(
            r.stderr.contains("syntax error"),
            "unclosed function: {:?}",
            r.stderr
        );

        // L131 unclosed double quote is handled (a result is produced).
        let r = shell().exec("echo \"unclosed");
        assert_ne!(
            r.exit_code, 0,
            "unclosed double quote produces an error result"
        );

        // L139 unclosed single quote is handled (a result is produced).
        let r = shell().exec("echo 'unclosed");
        assert_ne!(
            r.exit_code, 0,
            "unclosed single quote produces an error result"
        );

        // L156 redirect without target is handled (a result is produced).
        let r = shell().exec("echo test >");
        assert_ne!(
            r.exit_code, 0,
            "redirect without target produces an error result"
        );

        // L165 unknown command -> exit 127, "command not found".
        let r = shell().exec("unknowncommand");
        assert_eq!(r.exit_code, 127, "unknown command");
        assert!(
            r.stderr.contains("command not found"),
            "unknown command: {:?}",
            r.stderr
        );

        // L188 `local` outside a function -> exit 1, scoped error message.
        let r = shell().exec("local x=1");
        assert_eq!(r.exit_code, 1, "local outside function");
        assert!(
            r.stderr.contains("can only be used in a function"),
            "local outside function: {:?}",
            r.stderr
        );

        // L197 empty command before pipe is handled (a result is produced).
        let r = shell().exec("| cat");
        assert_ne!(
            r.exit_code, 0,
            "empty command before pipe produces an error result"
        );

        // L204 empty command after pipe is handled (a result is produced).
        let r = shell().exec("echo test |");
        assert_ne!(
            r.exit_code, 0,
            "empty command after pipe produces an error result"
        );

        // L210 `&&` with no second command is handled (a result is produced).
        let r = shell().exec("true &&");
        assert_ne!(
            r.exit_code, 0,
            "&& with no second command produces an error result"
        );

        // L216 `||` with no second command is handled (a result is produced).
        let r = shell().exec("false ||");
        assert_ne!(
            r.exit_code, 0,
            "|| with no second command produces an error result"
        );
    }

    /// Mirrors portable `packages/just-bash/src/syntax/set-errexit.test.ts`
    /// rows 1:1 through the Rust parser/interpreter. Each block asserts the
    /// upstream `it(...)` stdout/exit-code expectation on `Bash().exec(...)`.
    /// The `set -e`/`set +e`/`set -o errexit` toggles, the `&&`-final-failure,
    /// the `||`/`&&`-chain, the if/elif-condition, while/until-condition and
    /// -body, the negated-failed-command, and the preserve-exit-code rows are
    /// verified. The unimplemented `-ee`/`-ze`/`-ez` combined-flag rows (L270,
    /// L282, L291), the `false &&` short-circuit row (L100), the `! true` row
    /// (L147), and the `set` help/list/invalid-option rows (L317-L351) stay
    /// pending until those behaviors land.
    #[test]
    fn jbpi_syntax_set_errexit_match_upstream() {
        // L6 set -e exits immediately on the first failing command.
        let r = shell().exec("set -e\necho before\nfalse\necho after");
        assert_eq!(r.stdout, "before\n", "set -e exit");
        assert_eq!(r.exit_code, 1, "set -e exit code");

        // L18 without set -e execution continues past a failure.
        let r = shell().exec("echo before\nfalse\necho after");
        assert_eq!(r.stdout, "before\nafter\n", "no errexit");
        assert_eq!(r.exit_code, 0, "no errexit code");

        // L29 set -e does not exit when the command succeeds.
        let r = shell().exec("set -e\necho one\ntrue\necho two");
        assert_eq!(r.stdout, "one\ntwo\n", "errexit success");
        assert_eq!(r.exit_code, 0, "errexit success code");

        // L43 set +e disables errexit.
        let r = shell().exec("set -e\nset +e\necho before\nfalse\necho after");
        assert_eq!(r.stdout, "before\nafter\n", "set +e disables");
        assert_eq!(r.exit_code, 0, "set +e disables code");

        // L56 errexit can be re-enabled after set +e.
        let r = shell().exec("set -e\nset +e\nfalse\nset -e\necho before\nfalse\necho after");
        assert_eq!(r.stdout, "before\n", "re-enable errexit");
        assert_eq!(r.exit_code, 1, "re-enable errexit code");

        // L73 set -o errexit enables errexit.
        let r = shell().exec("set -o errexit\necho before\nfalse\necho after");
        assert_eq!(r.stdout, "before\n", "set -o errexit");
        assert_eq!(r.exit_code, 1, "set -o errexit code");

        // L85 set +o errexit disables errexit.
        let r = shell().exec("set -o errexit\nset +o errexit\necho before\nfalse\necho after");
        assert_eq!(r.stdout, "before\nafter\n", "set +o errexit");
        assert_eq!(r.exit_code, 0, "set +o errexit code");

        // L111 a failed command short-circuited by || does not trigger errexit.
        let r = shell().exec("set -e\nfalse || echo fallback\necho after");
        assert_eq!(r.stdout, "fallback\nafter\n", "|| short-circuit");
        assert_eq!(r.exit_code, 0, "|| short-circuit code");

        // L122 errexit triggers when the final command of an && list fails.
        let r = shell().exec("set -e\necho before\ntrue && false\necho after");
        assert_eq!(r.stdout, "before\n", "&& final failure");
        assert_eq!(r.exit_code, 1, "&& final failure code");

        // L134 a || rescue after a failed && does not trigger errexit.
        let r = shell().exec("set -e\nfalse && echo skip || echo fallback\necho after");
        assert_eq!(r.stdout, "fallback\nafter\n", "&& then ||");
        assert_eq!(r.exit_code, 0, "&& then || code");

        // L158 a negated failing command does not trigger errexit.
        let r = shell().exec("set -e\n! false\necho after");
        assert_eq!(r.stdout, "after\n", "negated false");
        assert_eq!(r.exit_code, 0, "negated false code");

        // L171 a failing command in an if condition does not trigger errexit.
        let r = shell().exec("set -e\nif false; then echo then; else echo else; fi\necho after");
        assert_eq!(r.stdout, "else\nafter\n", "if condition");
        assert_eq!(r.exit_code, 0, "if condition code");

        // L186 a failing command in an if body triggers errexit.
        let r = shell().exec("set -e\nif true; then echo body; false; echo nr; fi\necho after");
        assert_eq!(r.stdout, "body\n", "if body");
        assert_eq!(r.exit_code, 1, "if body code");

        // L201 a failing command in an elif condition does not trigger errexit.
        let r = shell().exec(
            "set -e\nif false; then echo one; elif false; then echo two; else echo three; fi\necho after",
        );
        assert_eq!(r.stdout, "three\nafter\n", "elif condition");
        assert_eq!(r.exit_code, 0, "elif condition code");

        // L220 a failing while condition that terminates the loop is exempt.
        let r = shell()
            .exec("set -e\nx=0\nwhile [ $x -lt 3 ]; do echo $x; x=$((x + 1)); done\necho after");
        assert_eq!(r.stdout, "0\n1\n2\nafter\n", "while condition");
        assert_eq!(r.exit_code, 0, "while condition code");

        // L235 a failing command in a while body triggers errexit.
        let r = shell().exec(
            "set -e\nx=0\nwhile [ $x -lt 3 ]; do echo $x; false; x=$((x + 1)); done\necho after",
        );
        assert_eq!(r.stdout, "0\n", "while body");
        assert_eq!(r.exit_code, 1, "while body code");

        // L253 a failing until condition that terminates the loop is exempt.
        let r = shell()
            .exec("set -e\nx=0\nuntil [ $x -ge 3 ]; do echo $x; x=$((x + 1)); done\necho after");
        assert_eq!(r.stdout, "0\n1\n2\nafter\n", "until condition");
        assert_eq!(r.exit_code, 0, "until condition code");

        // L306 the non-zero exit code from `exit` is preserved.
        let r = shell().exec("set -e\nexit 42");
        assert_eq!(r.exit_code, 42, "preserve exit code");
    }

    /// Covers additional portable
    /// `packages/just-bash/src/syntax/control-flow.test.ts` rows: if-body exit
    /// code (L73), `local`-scope isolation/restore/declare-without-value/nested
    /// (L222, L238, L261, L269), and `!` pipeline negation against grep
    /// (L313, L321). Each tuple mirrors an upstream `it(...)` stdout/exit-code
    /// assertion on `Bash().exec(...)`.
    #[test]
    fn jbpi_syntax_control_flow_function_local_and_negation_rows_match_upstream() {
        // L73 if returns the exit code of the last command in its body.
        let mut sh = shell();
        let r = sh.exec("if true; then echo hello; false; fi");
        assert_eq!(r.stdout, "hello\n", "L73 stdout");
        assert_eq!(r.exit_code, 1, "L73 exit");

        // L222 a `local x` inside a function does not leak to the outer scope.
        let mut sh = shell().with_env([("x", "outer")]);
        let r = sh.exec("test_func() { local x=inner; echo $x; }; test_func; echo $x");
        assert_eq!(r.stdout, "inner\nouter\n", "L222 stdout");
        assert_eq!(r.exit_code, 0, "L222 exit");

        // L238 a variable first introduced as `local` is unset after the call.
        let mut sh = shell();
        let r = sh.exec(
            "test_func() { local newvar=value; echo $newvar; }; test_func; echo \"[$newvar]\"",
        );
        assert_eq!(r.stdout, "value\n[]\n", "L238 stdout");
        assert_eq!(r.exit_code, 0, "L238 exit");

        // L261 `local x` without a value, then assigned in the same function.
        let mut sh = shell();
        let r = sh.exec("test_func() { local x; x=assigned; echo $x; }; test_func");
        assert_eq!(r.stdout, "assigned\n", "L261 stdout");
        assert_eq!(r.exit_code, 0, "L261 exit");

        // L269 nested function calls each keep their own `local x`.
        let mut sh = shell();
        let r = sh.exec(
            "inner() { local x=inner; echo $x; }; outer() { local x=outer; inner; echo $x; }; outer",
        );
        assert_eq!(r.stdout, "inner\nouter\n", "L269 stdout");
        assert_eq!(r.exit_code, 0, "L269 exit");

        // L313 `!` negates a failing grep to overall success.
        let mut sh = shell().with_files(ShellVirtualFileSystem::with_files([(
            "/test.txt",
            "hello world",
        )]));
        let r = sh.exec("! grep missing /test.txt");
        assert_eq!(r.exit_code, 0, "L313 exit");

        // L321 `!` negates a succeeding grep to overall failure.
        let mut sh = shell().with_files(ShellVirtualFileSystem::with_files([(
            "/test.txt",
            "hello world",
        )]));
        let r = sh.exec("! grep hello /test.txt > /dev/null");
        assert_eq!(r.exit_code, 1, "L321 exit");
    }

    /// Covers portable `packages/just-bash/src/interpreter/control-flow.test.ts`
    /// nested-control rows: quoted case pattern matches literally (L456), `if`
    /// inside `for` (L471), `for` inside `if` (L484), and `while` inside `case`
    /// (L513). Each tuple mirrors an upstream `it(...)` stdout/exit-code
    /// assertion on `Bash().exec(...)`.
    #[test]
    fn jbpi_interpreter_control_flow_nested_and_quoted_case_rows_match_upstream() {
        for (source, expected_stdout) in [
            // L456 a single-quoted case pattern matches the literal `*` value.
            (
                "x='*'\ncase $x in\n  '*') echo \"literal star\" ;;\n  *) echo \"default\" ;;\nesac",
                "literal star\n",
            ),
            // L471 `if` nested inside a `for` loop.
            (
                "for i in 1 2 3; do if [ $i -eq 2 ]; then echo \"found two\"; fi; done",
                "found two\n",
            ),
            // L484 `for` loop nested inside an `if` branch.
            (
                "x=1\nif [ $x -eq 1 ]; then for i in a b c; do echo $i; done; fi",
                "a\nb\nc\n",
            ),
            // L513 `while` loop nested inside a `case` arm.
            (
                "action=count\ncase $action in\n  count)\n    i=0\n    while [ $i -lt 3 ]; do\n      echo $i\n      i=$((i + 1))\n    done\n    ;;\nesac",
                "0\n1\n2\n",
            ),
        ] {
            let mut sh = shell();
            let result = sh.exec(source);
            assert_eq!(result.stderr, "", "stderr {source:?}");
            assert_eq!(result.stdout, expected_stdout, "stdout {source:?}");
            assert_eq!(result.exit_code, 0, "exit {source:?}");
        }
    }

    /// Covers portable `packages/just-bash/src/syntax/loops.test.ts` rows: a
    /// `for` loop without a semicolon before `do` (L184) and a malformed `for`
    /// header that is a syntax error (L200), through the Rust shell.
    #[test]
    fn jbpi_syntax_loops_for_without_semicolon_and_malformed_rows_match_upstream() {
        // L184 `for i in a b c do ...` (no `;` before `do`) still iterates.
        let mut sh = shell();
        let r = sh.exec("for i in a b c do echo $i; done");
        assert_eq!(r.stdout, "a\nb\nc\n", "L184 stdout");
        assert_eq!(r.exit_code, 0, "L184 exit");

        // L200 `for i a b c; do ...` (missing `in`) is a syntax error.
        let mut sh = shell();
        let r = sh.exec("for i a b c; do echo $i; done");
        assert_eq!(r.exit_code, 2, "L200 exit");
        assert!(
            r.stderr.contains("syntax error"),
            "L200 stderr {}",
            r.stderr
        );
    }

    /// Covers portable `packages/just-bash/src/syntax/parser-edge-cases.test.ts`
    /// rows for redirection without a space around the operator: `>` (L171) and
    /// `>>` (L178), verifying the written file contents through the Rust shell
    /// virtual filesystem.
    #[test]
    fn jbpi_syntax_parser_edge_cases_redirection_without_space_rows_match_upstream() {
        // L171 `echo hello>/tmp/test.txt` truncates and writes.
        let mut sh = shell();
        sh.exec("echo hello>/tmp/test.txt");
        assert_eq!(
            sh.files().read_to_string("/tmp/test.txt"),
            Some("hello\n"),
            "L171 file contents"
        );

        // L178 `echo second>>/tmp/test.txt` appends to the existing file.
        let mut sh = shell();
        sh.exec("echo first > /tmp/test.txt");
        sh.exec("echo second>>/tmp/test.txt");
        assert_eq!(
            sh.files().read_to_string("/tmp/test.txt"),
            Some("first\nsecond\n"),
            "L178 file contents"
        );
    }

    /// Covers portable `packages/just-bash/src/syntax/here-document.test.ts`
    /// whitespace-preservation rows: indented heredoc body (L173) and an
    /// ASCII-art triangle with leading spaces under a quoted delimiter (L188),
    /// through the Rust shell.
    #[test]
    fn jbpi_syntax_here_document_whitespace_preservation_rows_match_upstream() {
        // L173 heredoc body keeps its own indentation, independent of the
        // surrounding (indented) script.
        let mut sh = shell();
        let r = sh
            .exec("\n        cat <<EOF\n    indented content\n        more indented\nEOF\n      ");
        assert_eq!(
            r.stdout, "    indented content\n        more indented\n",
            "L173 stdout"
        );
        assert_eq!(r.exit_code, 0, "L173 exit");

        // L188 a quoted-delimiter heredoc preserves an ASCII-art triangle
        // with leading spaces verbatim.
        let art = "                    *\n                   * *\n                  *   *\n                 *     *\n                *       *\n               *         *\n              *           *\n             *             *\n            *               *\n           *                 *\n          *                   *\n         *********************\n";
        let mut sh = shell();
        let r = sh.exec(&format!("cat <<'EOF'\n{art}EOF"));
        assert_eq!(r.stdout, art, "L188 stdout");
        assert_eq!(r.exit_code, 0, "L188 exit");
    }

    /// Covers the portable `packages/just-bash/src/interpreter/helpers/xtrace.test.ts`
    /// L253 row: with `set -x`, commands run inside a command substitution are
    /// traced to stderr while the substitution output still flows to stdout.
    #[test]
    fn jbpi_interpreter_xtrace_command_substitution_row_matches_upstream() {
        let mut sh = shell();
        let r = sh.exec("set -x\nx=$(echo hello)\necho $x");
        assert_eq!(r.stdout, "hello\n", "L253 stdout");
        assert!(
            r.stderr.contains("echo hello"),
            "L253 traces the substitution command, stderr {}",
            r.stderr
        );
        assert_eq!(r.exit_code, 0, "L253 exit");
    }

    /// Covers the portable `packages/just-bash/src/interpreter/assoc-array.test.ts`
    /// L143 row: declaring an indexed array with `declare -a` and reading two
    /// numeric-index elements back, through the Rust shell.
    #[test]
    fn jbpi_interpreter_assoc_array_indexed_numeric_indices_row_matches_upstream() {
        let mut sh = shell();
        let r =
            sh.exec("declare -a arr\narr[0]=first\narr[1]=second\necho \"${arr[0]} ${arr[1]}\"");
        assert_eq!(r.stdout.trim(), "first second", "L143 stdout");
        assert_eq!(r.exit_code, 0, "L143 exit");
    }

    /// Covers portable `packages/just-bash/src/interpreter/builtins/set.test.ts`
    /// `set -u` (nounset) rows whose assertion is that NO unbound-variable error
    /// occurs and output is correct: set variable (L44), empty-string value
    /// (L55), disabled by `+u` (L66) and `+o nounset` (L87), special vars `$?`
    /// (L100), `$#` (L120), and `$@` (L130) not erroring, default/assign/alt
    /// parameter expansion (L170, L180, L191), and `set -eu` with a set variable
    /// (L203). Each tuple mirrors an upstream `it(...)` stdout/exit-code
    /// assertion on `Bash().exec(...)`.
    #[test]
    fn jbpi_interpreter_set_nounset_non_error_rows_match_upstream() {
        for (source, expected_stdout) in [
            // L44 a set variable is read without error under `set -u`.
            ("set -u\nMYVAR=hello\necho $MYVAR", "hello\n"),
            // L55 an empty-string value is a valid value, not "unbound".
            ("set -u\nMYVAR=\"\"\necho \"value: $MYVAR\"", "value: \n"),
            // L66 `set +u` disables nounset; unset var expands to empty.
            ("set -u\nset +u\necho $UNDEFINED", "\n"),
            // L87 `set +o nounset` likewise disables it.
            ("set -o nounset\nset +o nounset\necho $UNDEFINED", "\n"),
            // L100 `$?` is always defined and never triggers nounset.
            ("set -u\necho $?", "0\n"),
            // L120 `$#` is always defined and never triggers nounset.
            ("set -u\necho $#", "0\n"),
            // L130 `$@` with no positionals is empty, not "unbound".
            ("set -u\necho \"$@\"", "\n"),
            // L170 `${var:-default}` is allowed for an unset var under nounset.
            ("set -u\necho ${UNSET:-default}", "default\n"),
            // L180 `${var:=default}` assigns and is allowed under nounset.
            (
                "set -u\necho ${UNSET:=default}\necho $UNSET",
                "default\ndefault\n",
            ),
            // L191 `${var:+alt}` yields empty for an unset var under nounset.
            ("set -u\necho \":${UNSET:+alt}:\"", "::\n"),
            // L203 `set -eu` with a set variable runs without error.
            ("set -eu\nVAR=hello\necho $VAR", "hello\n"),
        ] {
            let mut sh = shell();
            let result = sh.exec(source);
            assert_eq!(result.stderr, "", "stderr {source:?}");
            assert_eq!(result.stdout, expected_stdout, "stdout {source:?}");
            assert_eq!(result.exit_code, 0, "exit {source:?}");
        }
    }

    /// Covers portable `packages/just-bash/src/interpreter/control-flow.test.ts`
    /// rows through the Rust shell interpreter. Each block mirrors one upstream
    /// `it(...)` assertion on `Bash().exec(...)` stdout/exit code. IFS-splitting,
    /// positional-parameter, invalid-identifier-error, and C-style `for (( ))`
    /// rows are intentionally excluded and stay pending until those behaviors are
    /// implemented.
    #[test]
    fn r2_interpreter_control_flow_rows_match_upstream() {
        for (source, expected_stdout) in [
            // L42 elif chain selects the matching branch.
            (
                "x=2\nif [ $x -eq 1 ]; then\n  echo \"one\"\nelif [ $x -eq 2 ]; then\n  echo \"two\"\nelif [ $x -eq 3 ]; then\n  echo \"three\"\nelse\n  echo \"other\"\nfi",
                "two\n",
            ),
            // L60 complex && condition between two test commands.
            (
                "a=5\nb=10\nif [ $a -lt $b ] && [ $b -gt 5 ]; then echo \"both true\"; fi",
                "both true\n",
            ),
            // L73 nested if statements.
            (
                "a=1\nb=2\nif [ $a -eq 1 ]; then if [ $b -eq 2 ]; then echo \"nested\"; fi; fi",
                "nested\n",
            ),
            // L247 while body runs while condition true.
            (
                "x=0\nwhile [ $x -lt 3 ]; do echo $x; x=$((x + 1)); done",
                "0\n1\n2\n",
            ),
            // L260 while body skipped when condition initially false.
            (
                "while false; do echo \"inside\"; done\necho \"done\"",
                "done\n",
            ),
            // L272 nested while loops.
            (
                "i=0\nwhile [ $i -lt 2 ]; do j=0; while [ $j -lt 2 ]; do echo \"$i,$j\"; j=$((j + 1)); done; i=$((i + 1)); done",
                "0,0\n0,1\n1,0\n1,1\n",
            ),
            // L291 until body runs until condition true.
            (
                "x=0\nuntil [ $x -ge 3 ]; do echo $x; x=$((x + 1)); done",
                "0\n1\n2\n",
            ),
            // L304 until body skipped when condition initially true.
            (
                "until true; do echo \"inside\"; done\necho \"done\"",
                "done\n",
            ),
            // L318 break exits the for loop early.
            (
                "for i in 1 2 3 4 5; do if [ $i -eq 3 ]; then break; fi; echo $i; done\necho done",
                "1\n2\ndone\n",
            ),
            // L331 continue skips one iteration.
            (
                "for i in 1 2 3 4 5; do if [ $i -eq 3 ]; then continue; fi; echo $i; done",
                "1\n2\n4\n5\n",
            ),
            // L343 break N exits multiple loop levels.
            (
                "for i in 1 2; do for j in a b c; do if [ $j = b ]; then break 2; fi; echo \"$i$j\"; done; done\necho done",
                "1a\ndone\n",
            ),
            // L358 continue N continues an outer loop level.
            (
                "for i in 1 2; do for j in a b; do if [ $j = a ]; then continue 2; fi; echo \"$i$j\"; done; done\necho done",
                "done\n",
            ),
        ] {
            let mut sh = shell();
            let result = sh.exec(source);
            assert_eq!(result.stderr, "", "stderr {source:?}");
            assert_eq!(result.stdout, expected_stdout, "stdout {source:?}");
            assert_eq!(result.exit_code, 0, "exit {source:?}");
        }
    }

    /// Covers additional portable `packages/just-bash/src/syntax/parser-edge-cases.test.ts`
    /// rows (quoting, operator parsing, edge cases, and `2>` / `2>&1` redirection)
    /// plus the operators.test.ts `||`-not-pipe row through the Rust shell. Rows
    /// requiring filesystem command families (mkdir/wc/head/tail) stay pending.
    #[test]
    fn r2_syntax_parser_edge_cases_and_redirection_rows_match_upstream() {
        for (source, expected_stdout, expected_code) in [
            // L30 adjacent quoted strings concatenate.
            ("echo 'hello'\"world\"", "helloworld\n", 0),
            // L68 escaped space outside quotes is a literal space.
            ("echo hello\\ world", "hello world\n", 0),
            // operators.test.ts L275 `||` is not confused with a pipe.
            ("cat /missing || echo fallback", "fallback\n", 0),
            // L247 mixed && and || left-to-right.
            ("false || echo A && echo B", "A\nB\n", 0),
            // L254 semicolon with && and ||.
            ("echo a; false || echo b; echo c", "a\nb\nc\n", 0),
            // L260 pipes with semicolons.
            ("echo hello | cat; echo world | cat", "hello\nworld\n", 0),
            // L266 assignment followed by command.
            ("x=hello; echo $x", "hello\n", 0),
            // L272 `a=b` as an argument, not an assignment.
            ("echo a=b", "a=b\n", 0),
            // L281 empty command line.
            ("", "", 0),
            // L288 command with only spaces.
            ("   ", "", 0),
            // L318 unicode in arguments.
            (
                "echo \"Hello \u{4e16}\u{754c} \u{1f30d}\"",
                "Hello \u{4e16}\u{754c} \u{1f30d}\n",
                0,
            ),
            // L324 literal newline preserved in double quotes.
            ("echo \"line1\nline2\"", "line1\nline2\n", 0),
        ] {
            let mut sh = shell();
            let result = sh.exec(source);
            assert_eq!(result.stdout, expected_stdout, "stdout {source:?}");
            assert_eq!(result.exit_code, expected_code, "exit {source:?}");
        }

        // L295 bare semicolon is a syntax error.
        let mut sh = shell();
        let result = sh.exec(";");
        assert_eq!(result.exit_code, 2, "bare semicolon exit");
        assert!(
            result.stderr.contains("syntax error"),
            "bare semicolon stderr {}",
            result.stderr
        );

        // L303 `;;` outside case is a syntax error.
        let mut sh = shell();
        let result = sh.exec("echo a;;;echo b");
        assert_eq!(result.exit_code, 2, "double semicolon exit");
        assert!(
            result.stderr.contains("syntax error"),
            "double semicolon stderr {}",
            result.stderr
        );

        // L311 very long argument round-trips intact.
        let long_str = "a".repeat(10000);
        let mut sh = shell();
        let result = sh.exec(&format!("echo {long_str}"));
        assert_eq!(result.stdout, format!("{long_str}\n"));

        // L186 `2>/dev/null` discards stderr, command still fails.
        let mut sh = shell();
        let result = sh.exec("cat /nonexistent 2>/dev/null");
        assert_eq!(result.stderr, "", "2>/dev/null stderr");
        assert_eq!(result.exit_code, 1, "2>/dev/null exit");

        // L193 `2>&1` routes stderr into stdout.
        let mut sh = shell();
        let result = sh.exec("cat /nonexistent 2>&1");
        assert!(
            result.stdout.contains("No such file"),
            "2>&1 stdout {}",
            result.stdout
        );
        assert_eq!(result.stderr, "", "2>&1 stderr");
    }

    /// Covers the portable `packages/just-bash/src/interpreter/arithmetic.test.ts`
    /// L487 arithmetic-command assignment row (`(( x = ... ))`) through the Rust
    /// shell. The division/modulo-by-zero and negative-exponent error rows stay
    /// pending until arithmetic error reporting is implemented.
    #[test]
    fn r2_interpreter_arithmetic_command_assignment_row_matches_upstream() {
        let mut sh = shell();
        let result = sh.exec("(( x = 5 + 3 )); echo $x");
        assert_eq!(result.stderr, "", "stderr");
        assert_eq!(result.stdout, "8\n", "stdout");
        assert_eq!(result.exit_code, 0, "exit");
    }

    /// Covers `packages/just-bash/src/syntax/parser-edge-cases.test.ts` quoting,
    /// escape-sequence, variable-expansion, whitespace, and operator-parsing rows
    /// through the Rust shell interpreter. Each tuple mirrors an upstream `it(...)`
    /// assertion on `Bash().exec(...)` stdout/exit code/stderr.
    #[test]
    fn jbr1_syntax_parser_edge_cases_match_upstream() {
        // Quoting rows (lines 6, 12, 18, 24, 36, 42) and escape rows
        // (lines 50, 56, 62, 74, 80) plus operator rows without spaces
        // (lines 208, 214, 220, 226) run on a fresh shell.
        for (source, expected_stdout) in [
            ("echo \"hello 'world'\"", "hello 'world'\n"),
            ("echo 'hello \"world\"'", "hello \"world\"\n"),
            ("echo \"\"", "\n"),
            ("echo ''", "\n"),
            ("echo foo'bar'baz", "foobarbaz\n"),
            ("echo '* ? | > < && || ;'", "* ? | > < && || ;\n"),
            ("echo \"hello \\\"world\\\"\"", "hello \"world\"\n"),
            ("echo \"a\\\\b\"", "a\\b\n"),
            ("echo \"\\$HOME\"", "$HOME\n"),
            ("echo 'a\\b'", "a\\b\n"),
            ("echo a\\|b", "a|b\n"),
            ("echo a&&echo b", "a\nb\n"),
            ("false||echo fallback", "fallback\n"),
            ("echo a;echo b", "a\nb\n"),
            ("echo hello|cat", "hello\n"),
            ("echo test | grep test || echo fail", "test\n"),
            ("true && echo success", "success\n"),
        ] {
            let mut sh = shell();
            let result = sh.exec(source);
            assert_eq!(result.stderr, "", "stderr {source}");
            assert_eq!(result.stdout, expected_stdout, "stdout {source}");
            assert_eq!(result.exit_code, 0, "exit {source}");
        }

        // Variable-expansion rows (lines 88, 94, 100, 106, 112, 118, 124, 130)
        // each need their own environment.
        let mut set_var = shell().with_env([("VAR", "value")]);
        assert_eq!(set_var.exec("echo \"${VAR:-default}\"").stdout, "value\n");
        let mut unset_var = shell();
        assert_eq!(
            unset_var.exec("echo \"${VAR:-default}\"").stdout,
            "default\n"
        );
        assert_eq!(unset_var.exec("echo \"${VAR:-}\"").stdout, "\n");
        let mut name_env = shell().with_env([("NAME", "test")]);
        assert_eq!(name_env.exec("echo $NAME").stdout, "test\n");
        let mut ab_env = shell().with_env([("A", "hello"), ("B", "world")]);
        assert_eq!(ab_env.exec("echo \"$A$B\"").stdout, "helloworld\n");
        let mut name_file = shell().with_env([("NAME", "test")]);
        assert_eq!(
            name_file.exec("echo \"${NAME}file.txt\"").stdout,
            "testfile.txt\n"
        );
        let mut undef = shell();
        assert_eq!(undef.exec("echo \"[$UNDEFINED]\"").stdout, "[]\n");
        let mut status_var = shell().with_env([("?", "0")]);
        assert_eq!(status_var.exec("echo \"$?\"").stdout, "0\n");

        // Whitespace rows (lines 139, 145, 151, 157, 163).
        let mut ws = shell();
        assert_eq!(ws.exec("echo    a    b    c").stdout, "a b c\n");
        assert_eq!(ws.exec("echo\ta\tb\tc").stdout, "a b c\n");
        assert_eq!(ws.exec("   echo hello").stdout, "hello\n");
        assert_eq!(ws.exec("echo hello   ").stdout, "hello\n");
        assert_eq!(
            ws.exec("echo \"  hello   world  \"").stdout,
            "  hello   world  \n"
        );
    }

    #[test]
    fn just_bash_parser_edge_cases_handles_adjacent_quoted_strings() {
        let result = shell().exec("echo 'hello'\"world\"");
        assert_eq!(result.stdout, "helloworld\n");
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn just_bash_parser_edge_cases_handles_escaped_space_outside_quotes() {
        let result = shell().exec("echo hello\\ world");
        assert_eq!(result.stdout, "hello world\n");
    }

    #[test]
    fn just_bash_variables_expands_default_and_adjacent_text() {
        let mut shell = shell().with_env([("SET", "value"), ("PREFIX", "pre")]);
        let result = shell.exec("echo ${MISSING:-default} ${SET:-fallback} ${PREFIX}fix");
        assert_eq!(result.stdout, "default value prefix\n");
    }

    #[test]
    fn just_bash_command_substitution_captures_nested_output() {
        let result = shell().exec("echo $(echo $(echo nested))");
        assert_eq!(result.stdout, "nested\n");
    }

    #[test]
    fn just_bash_command_substitution_converts_unquoted_newlines_to_spaces() {
        let mut shell = shell().with_files(ShellVirtualFileSystem::with_files([(
            "/test.txt",
            "line1\nline2\nline3",
        )]));
        let result = shell.exec("echo $(cat /test.txt)");
        assert_eq!(result.stdout, "line1 line2 line3\n");
    }

    #[test]
    fn just_bash_arithmetic_expansion_evaluates_precedence_and_variables() {
        let mut shell = shell().with_env([("X", "5")]);
        let result = shell.exec("echo $((2 + 3 * 4 - 1)) $((X + 3)) $((2 ** 8))");
        assert_eq!(result.stderr, "");
        assert_eq!(result.stdout, "13 8 256\n");
    }

    #[test]
    fn upstream_arithmetic_binary_comparison_logical_unary_and_variable_rows() {
        let mut shell = shell().with_env([("X", "5")]);

        assert_eq!(
            shell
                .exec("echo $((5 + 3)) $((10 - 4)) $((6 * 7)) $((20 / 4)) $((7 / 2)) $((17 % 5))")
                .stdout,
            "8 6 42 5 3 2\n"
        );
        assert_eq!(
            shell
                .exec("echo $((2 ** 10)) $((1 << 8)) $((256 >> 4)) $((12 & 10)) $((12 | 10)) $((12 ^ 10))")
                .stdout,
            "1024 256 16 8 14 6\n"
        );
        assert_eq!(
            shell
                .exec("echo $((3 < 5)) $((5 < 3)) $((3 <= 3)) $((4 <= 3)) $((5 > 3)) $((3 > 5))")
                .stdout,
            "1 0 1 0 1 0\n"
        );
        assert_eq!(
            shell
                .exec(
                    "echo $((3 >= 3)) $((2 >= 3)) $((5 == 5)) $((5 == 6)) $((5 != 6)) $((5 != 5))"
                )
                .stdout,
            "1 0 1 0 1 0\n"
        );
        assert_eq!(
            shell
                .exec("echo $((1 && 1)) $((1 && 0)) $((0 && 1)) $((1 || 0)) $((0 || 1)) $((0 || 0)) $((!0)) $((!1)) $((!5))")
                .stdout,
            "1 0 0 1 1 0 1 0 0\n"
        );
        assert_eq!(
            shell.exec("echo $((-5)) $((+5)) $((~0))").stdout,
            "-5 5 -1\n"
        );
        assert_eq!(
            shell
                .exec("x=5; echo $((x + 3)) $(($x + 3)) $((unset_var + 5)) $((2 * (3 + 4))) $(((1 + 2) * 3 + 4)) $((2 + 3 * 4)) $((X + 1))")
                .stdout,
            "8 8 5 14 13 14 6\n"
        );
        assert_eq!(shell.exec("(( 5 )); echo $?").stdout, "0\n");
        assert_eq!(shell.exec("(( 0 )); echo $?").stdout, "1\n");
    }

    #[test]
    fn upstream_arithmetic_comma_short_circuit_ternary_rows() {
        // comma operator: yields the last value
        assert_eq!(shell().exec("echo $((1, 2, 3))").stdout, "3\n");
        // short-circuit logical AND: rhs assignment must not run
        assert_eq!(
            shell().exec("x=5; echo $((0 && (x=10))); echo $x").stdout,
            "0\n5\n"
        );
        // short-circuit logical OR: rhs assignment must not run
        assert_eq!(
            shell().exec("x=5; echo $((1 || (x=10))); echo $x").stdout,
            "1\n5\n"
        );
        // ternary true / false / nested branches
        assert_eq!(shell().exec("echo $((1 ? 10 : 20))").stdout, "10\n");
        assert_eq!(shell().exec("echo $((0 ? 10 : 20))").stdout, "20\n");
        assert_eq!(shell().exec("echo $((1 ? 2 ? 3 : 4 : 5))").stdout, "3\n");
    }

    #[test]
    fn upstream_arithmetic_increment_decrement_rows() {
        assert_eq!(shell().exec("x=5; echo $((++x)); echo $x").stdout, "6\n6\n");
        assert_eq!(shell().exec("x=5; echo $((x++)); echo $x").stdout, "5\n6\n");
        assert_eq!(shell().exec("x=5; echo $((--x)); echo $x").stdout, "4\n4\n");
        assert_eq!(shell().exec("x=5; echo $((x--)); echo $x").stdout, "5\n4\n");
    }

    #[test]
    fn upstream_arithmetic_assignment_operator_rows() {
        assert_eq!(shell().exec("echo $((x = 5)); echo $x").stdout, "5\n5\n");
        assert_eq!(
            shell().exec("x=10; echo $((x += 5)); echo $x").stdout,
            "15\n15\n"
        );
        assert_eq!(
            shell().exec("x=10; echo $((x -= 3)); echo $x").stdout,
            "7\n7\n"
        );
        assert_eq!(
            shell().exec("x=4; echo $((x *= 3)); echo $x").stdout,
            "12\n12\n"
        );
        assert_eq!(
            shell().exec("x=20; echo $((x /= 4)); echo $x").stdout,
            "5\n5\n"
        );
        assert_eq!(
            shell().exec("x=17; echo $((x %= 5)); echo $x").stdout,
            "2\n2\n"
        );
        assert_eq!(
            shell().exec("x=2; echo $((x <<= 3)); echo $x").stdout,
            "16\n16\n"
        );
        assert_eq!(
            shell().exec("x=32; echo $((x >>= 2)); echo $x").stdout,
            "8\n8\n"
        );
        assert_eq!(
            shell().exec("x=12; echo $((x &= 10)); echo $x").stdout,
            "8\n8\n"
        );
        assert_eq!(
            shell().exec("x=12; echo $((x |= 1)); echo $x").stdout,
            "13\n13\n"
        );
        assert_eq!(
            shell().exec("x=12; echo $((x ^= 5)); echo $x").stdout,
            "9\n9\n"
        );
    }

    #[test]
    fn upstream_arithmetic_variable_resolution_and_base_rows() {
        // recursive variable name resolution: b=a, a=5 -> 5
        assert_eq!(shell().exec("a=5; b=a; echo $((b))").stdout, "5\n");
        // expressions stored in variables are re-evaluated
        assert_eq!(shell().exec("e='1+2'; echo $((e + 3))").stdout, "6\n");
        // number bases: octal, hex, base#number, hex-with-letters
        assert_eq!(shell().exec("echo $((010))").stdout, "8\n");
        assert_eq!(shell().exec("echo $((0xFF))").stdout, "255\n");
        assert_eq!(shell().exec("echo $((2#1010))").stdout, "10\n");
        assert_eq!(shell().exec("echo $((16#ff))").stdout, "255\n");
    }

    #[test]
    fn upstream_arithmetic_array_element_rows() {
        assert_eq!(
            shell().exec("arr=(10 20 30); echo $((arr[1] + 5))").stdout,
            "25\n"
        );
        assert_eq!(
            shell()
                .exec("arr=(0 0 0); echo $((arr[1] = 42)); echo ${arr[1]}")
                .stdout,
            "42\n42\n"
        );
        assert_eq!(
            shell()
                .exec("arr=(10 20 30); echo $((arr[0]++)); echo ${arr[0]}")
                .stdout,
            "10\n11\n"
        );
    }

    #[test]
    fn upstream_arithmetic_command_assignment_row() {
        // (( x = 5 + 3 )) assigns within the arithmetic command form
        assert_eq!(shell().exec("(( x = 5 + 3 )); echo $x").stdout, "8\n");
    }

    #[test]
    fn just_bash_loops_for_iterates_list_items() {
        let result = shell().exec("for i in a b c; do echo $i; done");
        assert_eq!(result.stdout, "a\nb\nc\n");
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn upstream_control_flow_if_for_case_rows() {
        let mut shell = shell();

        assert_eq!(shell.exec("if true; then\n  echo yes\nfi").stdout, "yes\n");
        assert_eq!(
            shell
                .exec("if false; then\n  echo yes\nfi\necho \"done\"")
                .stdout,
            "done\n"
        );
        assert_eq!(
            shell
                .exec("if false; then\n  echo yes\nelse\n  echo no\nfi")
                .stdout,
            "no\n"
        );
        assert_eq!(
            shell
                .exec("items=\"x y z\"; for i in a b c; do echo $i; done; for i in $items; do echo $i; done")
                .stdout,
            "a\nb\nc\nx\ny\nz\n"
        );
        assert_eq!(
            shell
                .exec("for i in; do echo $i; done; echo \"done\"; for i in 1 2 3; do :; done; echo $i; for i in {1..3}; do echo $i; done")
                .stdout,
            "done\n3\n1\n2\n3\n"
        );
        assert_eq!(
            shell
                .exec("x=hello; case $x in hello) echo \"matched hello\" ;; world) echo \"matched world\" ;; esac")
                .stdout,
            "matched hello\n"
        );
        assert_eq!(
            shell
                .exec("x=hello; case $x in h*) echo \"starts with h\" ;; *) echo default ;; esac")
                .stdout,
            "starts with h\n"
        );
        assert_eq!(
            shell
                .exec(
                    "x=yes; case $x in yes|y|Y) echo affirmative ;; no|n|N) echo negative ;; esac"
                )
                .stdout,
            "affirmative\n"
        );
        assert_eq!(
            shell
                .exec(
                    "x=unknown; case $x in yes) echo yes ;; no) echo no ;; *) echo default ;; esac"
                )
                .stdout,
            "default\n"
        );
        assert_eq!(
            shell
                .exec("for x in foo bar baz; do case $x in foo) echo one ;; bar) echo two ;; *) echo other ;; esac; done")
                .stdout,
            "one\ntwo\nother\n"
        );
    }

    #[test]
    fn just_bash_control_flow_if_elif_else_uses_command_status() {
        let result =
            shell().exec("if false; then echo one; elif true; then echo two; else echo three; fi");
        assert_eq!(result.stdout, "two\n");
    }

    #[test]
    fn just_bash_functions_define_call_and_pass_arguments() {
        let result = shell().exec("greet() { echo Hello $1; }; greet World");
        assert_eq!(result.stdout, "Hello World\n");
    }

    #[test]
    fn just_bash_operators_mixed_and_or_are_left_associative() {
        let result = shell().exec("false || echo A && echo B");
        assert_eq!(result.stdout, "A\nB\n");
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn just_bash_pipeline_status_respects_pipefail() {
        let mut shell = shell();
        let default_result = shell.exec("false | true; echo \"exit: $?\"");
        assert_eq!(default_result.stdout, "exit: 0\n");
        let pipefail_result = shell.exec("set -o pipefail; false | true; echo \"exit: $?\"");
        assert_eq!(pipefail_result.stdout, "exit: 1\n");
    }

    #[test]
    fn upstream_set_pipefail_rows() {
        let mut interp = shell();

        assert_eq!(
            interp
                .exec("set -o pipefail; echo hello | cat | cat; echo \"exit: $?\"")
                .stdout,
            "hello\nexit: 0\n"
        );
        assert_eq!(
            interp
                .exec("set -o pipefail; false | true; echo \"exit: $?\"")
                .stdout,
            "exit: 1\n"
        );
        assert_eq!(
            interp
                .exec("set -o pipefail; echo hello | false | cat; echo \"exit: $?\"")
                .stdout,
            "exit: 1\n"
        );
        let mut no_pipefail_shell = shell();
        assert_eq!(
            no_pipefail_shell
                .exec("false | true; echo \"exit: $?\"")
                .stdout,
            "exit: 0\n"
        );
        assert_eq!(
            interp
                .exec("set +o pipefail; false | true; echo \"exit: $?\"")
                .stdout,
            "exit: 0\n"
        );
    }

    /// Covers portable `packages/just-bash/src/interpreter/builtins/set.test.ts`
    /// errexit-exception, pipefail, and nounset-positional rows that exercise
    /// the `set -e` / `set -o pipefail` / `set -u` interaction with control flow
    /// through the Rust shell interpreter. Each tuple mirrors one upstream
    /// `it(...)` assertion on `Bash().exec(...)` stdout/exit code:
    ///   - L325 errexit aborts on the FINAL failing member of an `&&` list.
    ///   - L337 errexit ignores a `!`-negated failed command.
    ///   - L348 a failed `if` CONDITION does not trip errexit (else runs).
    ///   - L363 a failed command in an `if` BODY trips errexit.
    ///   - L378 a `while` condition becoming false does not trip errexit.
    ///   - L393 a failed command in a `while` BODY trips errexit.
    ///   - L444 pipefail returns the rightmost failing exit code.
    ///   - L477 errexit + pipefail aborts when the pipeline fails.
    ///   - L155 a SET positional parameter is read without nounset error.
    #[test]
    fn upstream_set_errexit_exception_pipefail_positional_rows() {
        // (source, expected_stdout, expected_exit_code)
        for (source, expected_stdout, expected_exit) in [
            // L325 the final command of an `&&` list fails -> errexit aborts.
            (
                "set -e\necho before\ntrue && false\necho after",
                "before\n",
                1,
            ),
            // L337 a `!`-negated failed command yields success -> no abort.
            ("set -e\n! false\necho after", "after\n", 0),
            // L348 a failed `if` condition selects else and does not abort.
            (
                "set -e\nif false; then\n  echo \"then\"\nelse\n  echo \"else\"\nfi\necho after",
                "else\nafter\n",
                0,
            ),
            // L363 a failed command inside the `if` body aborts the script.
            (
                "set -e\nif true; then\n  echo \"in body\"\n  false\n  echo \"not reached\"\nfi\necho after",
                "in body\n",
                1,
            ),
            // L378 a `while` condition becoming false terminates the loop
            // normally without tripping errexit.
            (
                "set -e\nx=0\nwhile [ $x -lt 3 ]; do\n  echo $x\n  x=$((x + 1))\ndone\necho after",
                "0\n1\n2\nafter\n",
                0,
            ),
            // L393 a failed command inside the `while` body aborts the script.
            (
                "set -e\nx=0\nwhile [ $x -lt 3 ]; do\n  echo $x\n  false\n  x=$((x + 1))\ndone\necho after",
                "0\n",
                1,
            ),
            // L444 pipefail returns the RIGHTMOST failing exit code (3, not 2).
            (
                "set -o pipefail\nexit 2 | exit 3 | true\necho \"exit: $?\"",
                "exit: 3\n",
                0,
            ),
            // L477 errexit + pipefail: a failing pipeline aborts the script.
            (
                "set -e\nset -o pipefail\necho before\nfalse | true\necho after",
                "before\n",
                1,
            ),
            // L155 under nounset, a SET positional parameter reads cleanly.
            (
                "myfunc() {\n  set -u\n  echo $1\n}\nmyfunc hello",
                "hello\n",
                0,
            ),
        ] {
            let mut sh = shell();
            let result = sh.exec(source);
            assert_eq!(result.stdout, expected_stdout, "stdout {source:?}");
            assert_eq!(result.exit_code, expected_exit, "exit {source:?}");
        }
    }

    /// Covers portable `packages/just-bash/src/interpreter/builtins/set.test.ts`
    /// "set error handling" rows through the Rust shell interpreter:
    ///   - L492 `set --help` succeeds and prints usage including `-e`.
    ///   - L500 `set -z` is an "invalid option" error mentioning `-z` (exit 1).
    ///   - L508 `set -o unknownoption` is an "invalid option name" error
    ///     mentioning `unknownoption` (exit 1).
    ///   - L516 `set -o` (no argument) lists options including `errexit`.
    #[test]
    fn upstream_set_error_handling_rows() {
        // L492 `set --help`.
        let help = shell().exec("set --help");
        assert_eq!(help.exit_code, 0, "L492 exit");
        assert!(
            help.stdout.contains("usage:"),
            "L492 usage {:?}",
            help.stdout
        );
        assert!(help.stdout.contains("-e"), "L492 -e {:?}", help.stdout);

        // L500 unknown short option.
        let short = shell().exec("set -z");
        assert_eq!(short.exit_code, 1, "L500 exit");
        assert!(short.stderr.contains("-z"), "L500 -z {:?}", short.stderr);
        assert!(
            short.stderr.contains("invalid option"),
            "L500 invalid option {:?}",
            short.stderr
        );

        // L508 unknown long option.
        let long = shell().exec("set -o unknownoption");
        assert_eq!(long.exit_code, 1, "L508 exit");
        assert!(
            long.stderr.contains("unknownoption"),
            "L508 name {:?}",
            long.stderr
        );
        assert!(
            long.stderr.contains("invalid option name"),
            "L508 invalid option name {:?}",
            long.stderr
        );

        // L516 `set -o` lists options.
        let list = shell().exec("set -o");
        assert_eq!(list.exit_code, 0, "L516 exit");
        assert!(
            list.stdout.contains("errexit"),
            "L516 errexit {:?}",
            list.stdout
        );
    }

    #[test]
    fn just_bash_redirection_writes_virtual_files_without_host_shell() {
        let mut shell = shell();
        let result =
            shell.exec("echo first > /tmp/out.txt; echo second >> /tmp/out.txt; cat /tmp/out.txt");
        assert_eq!(result.stdout, "first\nsecond\n");
        assert_eq!(
            shell.files().read_to_string("/tmp/out.txt"),
            Some("first\nsecond\n")
        );
    }

    #[test]
    fn just_bash_arrays_assignment_and_index_expansion() {
        let result =
            shell().exec("declare -a arr; arr[0]=first; arr[1]=second; echo ${arr[0]} ${arr[1]}");
        assert_eq!(result.stdout, "first second\n");
    }

    #[test]
    fn upstream_export_builtin_assignment_listing_and_same_exec_rows() {
        let mut shell = shell().with_env([("EXISTING", "value"), ("FOO", "bar")]);

        assert_eq!(shell.exec("export FOO=bar; echo $FOO").stdout, "bar\n");
        assert_eq!(
            shell.exec("export FOO=bar BAZ=qux; echo $FOO $BAZ").stdout,
            "bar qux\n"
        );
        assert_eq!(
            shell
                .exec("export URL=http://example.com?foo=bar; echo $URL")
                .stdout,
            "http://example.com?foo=bar\n"
        );
        assert_eq!(shell.exec("export EMPTY; echo X${EMPTY}X").stdout, "XX\n");
        assert_eq!(
            shell.exec("export EXISTING; echo $EXISTING").stdout,
            "value\n"
        );
        assert_eq!(
            shell
                .exec("export GREETING=hello; echo $GREETING world")
                .stdout,
            "hello world\n"
        );
        assert_eq!(shell.exec("export FOO=bar; (echo $FOO)").stdout, "bar\n");
        assert_eq!(shell.exec("export -n FOO; echo \"$FOO\"").stdout, "bar\n");
        assert_eq!(
            shell.exec("export -n FOO BAZ; echo \"$FOO $BAZ\"").stdout,
            "bar qux\n"
        );

        let listing = shell.exec("export").stdout;
        assert!(listing.contains("declare -x FOO=\"bar\""));
        assert!(listing.contains("declare -x BAZ=\"qux\""));

        let p_listing = shell.exec("export -p").stdout;
        assert!(p_listing.contains("declare -x FOO=\"bar\""));

        let quoted_listing = shell.exec("export MSG=\"it's working\"; export").stdout;
        assert!(quoted_listing.contains("declare -x MSG=\"it's working\""));

        let alias_listing = shell.exec("alias ll='ls -la'; export").stdout;
        assert!(!alias_listing.contains("BASH_ALIAS"));
        assert!(alias_listing.contains("declare -x FOO=\"bar\""));
    }

    #[test]
    fn upstream_unset_builtin_variable_function_and_scope_rows() {
        let mut interp = shell().with_env([("VAR", "value"), ("A", "1"), ("B", "2"), ("C", "3")]);

        assert_eq!(
            interp
                .exec("echo \"before: $VAR\"; unset VAR; echo \"after: $VAR\"")
                .stdout,
            "before: value\nafter: \n"
        );
        assert_eq!(
            interp.exec("unset A B; echo \"A=$A B=$B C=$C\"").stdout,
            "A= B= C=3\n"
        );
        assert_eq!(
            interp.exec("unset NONEXISTENT; echo \"done\"").stdout,
            "done\n"
        );
        assert_eq!(
            interp
                .with_env([("VAR", "value")])
                .exec("unset -v VAR; echo \"VAR=$VAR\"")
                .stdout,
            "VAR=\n"
        );
        let mut status_shell = shell().with_env([("VAR", "value")]);
        assert_eq!(status_shell.exec("unset VAR; echo $?").stdout, "0\n");

        let mut function_shell = shell();
        let function_result =
            function_shell.exec("myfunc() { echo hello; }; myfunc; unset -f myfunc; myfunc");
        assert_eq!(function_result.stdout, "hello\n");
        assert!(function_result.stderr.contains("myfunc: command not found"));
        assert_eq!(
            function_shell
                .exec("unset -f nonexistent_func; echo \"done\"")
                .stdout,
            "done\n"
        );

        let mut scoped_shell = shell().with_env([("VAR", "outer")]);
        assert_eq!(
            scoped_shell
                .exec("myfunc() { unset VAR; echo \"in func: $VAR\"; }; myfunc; echo \"outside: $VAR\"")
                .stdout,
            "in func: \noutside: \n"
        );
        assert_eq!(
            scoped_shell
                .exec("myfunc() { local VAR=local; echo \"before: $VAR\"; unset VAR; echo \"after: $VAR\"; }; myfunc")
                .stdout,
            "before: local\nafter: \n"
        );
        assert_eq!(
            scoped_shell
                .exec("VAR=value; unset VAR; echo \"done\"")
                .stdout,
            "done\n"
        );
    }

    /// Mirrors `packages/just-bash/src/interpreter/builtins/eval.test.ts`
    /// portable rows 1:1 through the Rust parser/interpreter `Bash().exec`.
    /// Covers basic evaluation (L6/L13/L19/L26), variable expansion before
    /// execution including dynamic names and dynamic assignment
    /// (L35/L44/L54), command construction over expanded word lists and
    /// command substitution (L66/L81), exit-code propagation of the last
    /// command (L91/L97), the parse-error row (L101), current-environment and
    /// function visibility/definition (L112/L122/L131), and single/double
    /// quote handling (L142/L148). The piped row (L75) stays pending because
    /// `tr` is not provided by the parser/interpreter command seam.
    #[test]
    fn jbpi_interpreter_builtin_eval_matches_upstream() {
        // L6 simple command.
        let r = shell().exec("eval \"echo hello\"");
        assert_eq!(r.stdout, "hello\n");
        assert_eq!(r.exit_code, 0);
        // L13 multiple words as a single command.
        assert_eq!(
            shell().exec("eval echo hello world").stdout,
            "hello world\n"
        );
        // L19 empty argument is a success no-op.
        let r = shell().exec("eval \"\"");
        assert_eq!(r.stdout, "");
        assert_eq!(r.exit_code, 0);
        // L26 no arguments is a success no-op.
        let r = shell().exec("eval");
        assert_eq!(r.stdout, "");
        assert_eq!(r.exit_code, 0);
        // L35 expands variables before execution.
        assert_eq!(
            shell().exec("cmd=\"echo hello\"\neval $cmd").stdout,
            "hello\n"
        );
        // L44 dynamic variable names.
        assert_eq!(
            shell()
                .exec("name=\"FOO\"\nFOO=\"bar\"\neval \"echo \\$$name\"")
                .stdout,
            "bar\n"
        );
        // L54 dynamic variable assignment.
        assert_eq!(
            shell()
                .exec("name=\"MYVAR\"\neval \"$name=hello\"\necho $MYVAR")
                .stdout,
            "hello\n"
        );
        // L66 command construction over an expanded word list.
        assert_eq!(
            shell()
                .exec("args=\"a b c\"\neval \"for x in $args; do echo item: \\$x; done\"")
                .stdout,
            "item: a\nitem: b\nitem: c\n"
        );
        // L81 command substitution inside eval.
        assert_eq!(
            shell().exec("eval \"echo $(echo nested)\"").stdout,
            "nested\n"
        );
        // L91 returns the exit code of the executed command.
        assert_eq!(shell().exec("eval false").exit_code, 1);
        // L97 returns the exit code of the last command.
        assert_eq!(shell().exec("eval \"true; false; true\"").exit_code, 0);
        // L101 parse error: exit 1 and "Parse error" on stderr.
        let r = shell().exec("eval \"for do done\"");
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("Parse error"), "stderr: {:?}", r.stderr);
        // L112 executes in the current environment.
        assert_eq!(
            shell()
                .exec("FOO=original\neval \"FOO=modified\"\necho $FOO")
                .stdout,
            "modified\n"
        );
        // L122 has access to existing functions.
        assert_eq!(
            shell()
                .exec("myfunc() { echo \"called\"; }\neval \"myfunc\"")
                .stdout,
            "called\n"
        );
        // L131 defines functions that persist after eval.
        assert_eq!(
            shell()
                .exec("eval 'greet() { echo \"hello $1\"; }'\ngreet world")
                .stdout,
            "hello world\n"
        );
        // L142 single quotes.
        assert_eq!(
            shell().exec("eval \"echo 'single quoted'\"").stdout,
            "single quoted\n"
        );
        // L148 double quotes.
        assert_eq!(
            shell().exec("eval 'echo \"double quoted\"'").stdout,
            "double quoted\n"
        );
    }

    /// Mirrors `packages/just-bash/src/interpreter/builtins/return.test.ts`
    /// portable rows 1:1 through the Rust parser/interpreter `Bash().exec`.
    /// Covers basic return with the default/explicit/last-command status and
    /// status 0 (L6/L20/L31/L43), modulo-256 wrapping including the negative
    /// row (L57/L69/L80), the not-in-a-function and non-numeric error rows
    /// (L94/L101), innermost-only return and propagation through control flow
    /// (L116/L132), and stdout preservation before return (L148).
    #[test]
    fn jbpi_interpreter_builtin_return_matches_upstream() {
        // L6 default exit code stops the function body but not the caller.
        assert_eq!(
            shell()
                .exec("myfunc() {\necho before\nreturn\necho after\n}\nmyfunc\necho done")
                .stdout,
            "before\ndone\n"
        );
        // L20 explicit return code surfaces via $?.
        assert_eq!(
            shell()
                .exec("myfunc() { return 42; }\nmyfunc\necho $?")
                .stdout,
            "42\n"
        );
        // L31 no argument uses the last command's exit code.
        assert_eq!(
            shell()
                .exec("myfunc() { false; return; }\nmyfunc\necho $?")
                .stdout,
            "1\n"
        );
        // L43 explicit return 0.
        assert_eq!(
            shell()
                .exec("myfunc() { return 0; }\nmyfunc\necho $?")
                .stdout,
            "0\n"
        );
        // L57 return 256 wraps to 0.
        assert_eq!(
            shell()
                .exec("myfunc() { return 256; }\nmyfunc\necho $?")
                .stdout,
            "0\n"
        );
        // L69 return 257 wraps to 1.
        assert_eq!(
            shell()
                .exec("myfunc() { return 257; }\nmyfunc\necho $?")
                .stdout,
            "1\n"
        );
        // L80 return -1 wraps to 255.
        assert_eq!(
            shell()
                .exec("myfunc() { return -1; }\nmyfunc\necho $?")
                .stdout,
            "255\n"
        );
        // L94 return outside a function is an error (exit 1).
        let r = shell().exec("return");
        assert!(
            r.stderr
                .contains("can only `return' from a function or sourced script"),
            "stderr: {:?}",
            r.stderr
        );
        assert_eq!(r.exit_code, 1);
        // L101 non-numeric argument is a status-2 error.
        let r = shell().exec("myfunc() { return abc; }\nmyfunc");
        assert!(
            r.stderr.contains("numeric argument required"),
            "stderr: {:?}",
            r.stderr
        );
        assert_eq!(r.exit_code, 2);
        // L116 only the innermost function returns.
        assert_eq!(
            shell()
                .exec("outer() {\necho outer-start\ninner() {\necho inner\nreturn 5\n}\ninner\necho \"inner returned $?\"\n}\nouter\necho \"outer returned $?\"")
                .stdout,
            "outer-start\ninner\ninner returned 5\nouter returned 0\n"
        );
        // L132 return propagates through control flow inside the function.
        assert_eq!(
            shell()
                .exec("myfunc() { for i in 1 2 3; do if [ $i -eq 2 ]; then return 42; fi; echo $i; done; echo never; }\nmyfunc\necho $?")
                .stdout,
            "1\n42\n"
        );
        // L148 stdout produced before return is preserved.
        assert_eq!(
            shell()
                .exec("myfunc() { echo line1; echo line2; return 3; }\nmyfunc\necho \"exit: $?\"")
                .stdout,
            "line1\nline2\nexit: 3\n"
        );
    }

    /// Mirrors every row of
    /// `packages/just-bash/src/interpreter/builtins/shift.test.ts` 1:1 through
    /// the Rust parser/interpreter `Bash().exec`. Covers basic shift by 1 and
    /// the resulting `$1 $2 $3`/`$#`/`$@` updates (L6/L19/L32), shift with an
    /// explicit count and `shift 0` no-op (L47/L60/L72), the count-out-of-range
    /// and numeric-argument-required error rows (L87/L99/L111), consecutive
    /// shifts and the `while [ $# -gt 0 ]` loop (L125/L140), function-scope
    /// isolation in nested functions (L156), and the no-parameter and
    /// single-parameter edge rows (L174/L186).
    #[test]
    fn jbpi_interpreter_builtin_shift_matches_upstream() {
        // L6 shift positional parameters by 1.
        assert_eq!(
            shell()
                .exec("myfunc() {\necho \"before: $1 $2 $3\"\nshift\necho \"after: $1 $2 $3\"\n}\nmyfunc a b c")
                .stdout,
            "before: a b c\nafter: b c \n"
        );
        // L19 $# is updated after shift.
        assert_eq!(
            shell()
                .exec("myfunc() {\necho \"count: $#\"\nshift\necho \"count: $#\"\n}\nmyfunc a b c")
                .stdout,
            "count: 3\ncount: 2\n"
        );
        // L32 $@ is updated after shift.
        assert_eq!(
            shell()
                .exec("myfunc() {\necho \"args: $@\"\nshift\necho \"args: $@\"\n}\nmyfunc a b c")
                .stdout,
            "args: a b c\nargs: b c\n"
        );
        // L47 shift by a specified count.
        assert_eq!(
            shell()
                .exec("myfunc() {\necho \"before: $1 $2 $3 $4\"\nshift 2\necho \"after: $1 $2\"\n}\nmyfunc a b c d")
                .stdout,
            "before: a b c d\nafter: c d\n"
        );
        // L60 shift all parameters.
        assert_eq!(
            shell()
                .exec("myfunc() {\nshift 3\necho \"count: $#\"\n}\nmyfunc a b c")
                .stdout,
            "count: 0\n"
        );
        // L72 shift 0 is a no-op with status 0.
        let r = shell().exec("myfunc() {\nshift 0\necho \"$1 $2\"\n}\nmyfunc a b");
        assert_eq!(r.stdout, "a b\n");
        assert_eq!(r.exit_code, 0);
        // L87 shift count exceeding the parameter count is an error.
        let r = shell().exec("myfunc() {\nshift 5\n}\nmyfunc a b c");
        assert!(
            r.stderr.contains("shift count out of range"),
            "{:?}",
            r.stderr
        );
        assert_eq!(r.exit_code, 1);
        // L99 negative count is a numeric-argument error.
        let r = shell().exec("myfunc() {\nshift -1\n}\nmyfunc a b");
        assert!(
            r.stderr.contains("numeric argument required"),
            "{:?}",
            r.stderr
        );
        assert_eq!(r.exit_code, 1);
        // L111 non-numeric argument is a numeric-argument error.
        let r = shell().exec("myfunc() {\nshift abc\n}\nmyfunc a b");
        assert!(
            r.stderr.contains("numeric argument required"),
            "{:?}",
            r.stderr
        );
        assert_eq!(r.exit_code, 1);
        // L125 consecutive shifts walk the parameter list.
        assert_eq!(
            shell()
                .exec("myfunc() {\necho $1\nshift\necho $1\nshift\necho $1\n}\nmyfunc a b c")
                .stdout,
            "a\nb\nc\n"
        );
        // L140 shift in a while loop drains the parameters.
        assert_eq!(
            shell()
                .exec("myfunc() {\nwhile [ $# -gt 0 ]; do\necho $1\nshift\ndone\n}\nmyfunc x y z")
                .stdout,
            "x\ny\nz\n"
        );
        // L156 shift only affects the current function scope.
        assert_eq!(
            shell()
                .exec("outer() {\ninner() {\nshift\necho \"inner: $1\"\n}\ninner x y z\necho \"outer: $1\"\n}\nouter a b c")
                .stdout,
            "inner: y\nouter: a\n"
        );
        // L174 shift with no parameters is a count-out-of-range error.
        let r = shell().exec("myfunc() {\nshift\n}\nmyfunc");
        assert!(
            r.stderr.contains("shift count out of range"),
            "{:?}",
            r.stderr
        );
        assert_eq!(r.exit_code, 1);
        // L186 shift with a single parameter leaves $1 empty.
        assert_eq!(
            shell()
                .exec("myfunc() {\necho \"before: $1\"\nshift\necho \"after: $1\"\n}\nmyfunc only")
                .stdout,
            "before: only\nafter: \n"
        );
    }

    /// Mirrors the previously-pending rows of
    /// `packages/just-bash/src/interpreter/builtins/exit.test.ts` through the
    /// Rust parser/interpreter `Bash().exec`: exit from inside a for loop
    /// (L72), exit from inside an if block (L85), and the no-argument rows
    /// that resolve to the last command's status (L101 after `false`, L110
    /// after `true`). The remaining exit rows are covered by the exec-engine
    /// `builtins_exit_*` tests.
    #[test]
    fn jbpi_interpreter_builtin_exit_context_and_last_status_rows() {
        // L72 exit from a loop stops the loop and the script.
        let r = shell().exec("for i in 1 2 3; do echo $i; exit 10; done\necho never");
        assert_eq!(r.stdout, "1\n");
        assert_eq!(r.exit_code, 10);
        // L85 exit from an if block stops the script.
        let r = shell().exec("if true; then echo in; exit 7; echo never; fi\necho never2");
        assert_eq!(r.stdout, "in\n");
        assert_eq!(r.exit_code, 7);
        // L101 no-arg exit uses the last status (false -> 1).
        assert_eq!(shell().exec("false\nexit").exit_code, 1);
        // L110 no-arg exit uses the last status (true -> 0).
        assert_eq!(shell().exec("true\nexit").exit_code, 0);
    }

    #[test]
    fn just_bash_case_statement_matches_first_pattern() {
        let result = shell()
            .exec("case test in test) echo first ;; test) echo second ;; *) echo other ;; esac");
        assert_eq!(result.stdout, "first\n");
    }

    #[test]
    fn just_bash_alias_expansion_uses_dispatch_seam() {
        let mut shell = shell();
        shell.state_mut().set_alias("ll", "echo listed");
        let result = shell.exec("ll /tmp");
        assert_eq!(result.stdout, "listed /tmp\n");
    }

    #[test]
    fn just_bash_alias_lists_no_aliases_initially() {
        let result = shell().exec("alias");
        assert_eq!(result.stdout, "");
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn just_bash_alias_sets_and_lists_within_same_exec() {
        let result = shell().exec("alias ll='ls -la'; alias");
        assert_eq!(result.stdout, "alias ll='ls -la'\n");
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn just_bash_alias_shows_specific_alias_within_same_exec() {
        let result = shell().exec("alias ll='ls -la'; alias ll");
        assert_eq!(result.stdout, "alias ll='ls -la'\n");
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn just_bash_alias_errors_when_alias_not_found() {
        let result = shell().exec("alias notexists");
        assert!(result.stderr.contains("not found"));
        assert_eq!(result.exit_code, 1);
    }

    #[test]
    fn just_bash_alias_sets_multiple_within_same_exec_in_definition_order() {
        let result = shell().exec("alias ll='ls -la' la='ls -a'; alias");
        assert_eq!(result.stdout, "alias ll='ls -la'\nalias la='ls -a'\n");
    }

    #[test]
    fn just_bash_alias_shows_help_with_help_flag() {
        let result = shell().exec("alias --help");
        assert!(result.stdout.contains("alias"));
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn just_bash_alias_does_not_persist_across_exec_calls() {
        let mut shell = shell();
        shell.exec("alias ll='ls -la'");
        let result = shell.exec("alias ll");
        assert!(result.stderr.contains("not found"));
        assert_eq!(result.exit_code, 1);
    }

    #[test]
    fn just_bash_unalias_removes_an_alias_within_same_exec() {
        let result = shell().exec("alias ll='ls -la'; unalias ll; alias ll");
        assert!(result.stderr.contains("not found"));
        assert_eq!(result.exit_code, 1);
    }

    #[test]
    fn just_bash_unalias_errors_when_unaliasing_nonexistent_alias() {
        let result = shell().exec("unalias notexists");
        assert!(result.stderr.contains("not found"));
        assert_eq!(result.exit_code, 1);
    }

    #[test]
    fn just_bash_unalias_removes_all_aliases_with_a_flag() {
        let result = shell().exec("alias ll='ls -la' la='ls -a'; unalias -a; alias");
        assert_eq!(result.stdout, "");
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn just_bash_unalias_shows_help_with_help_flag() {
        let result = shell().exec("unalias --help");
        assert!(result.stdout.contains("unalias"));
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn just_bash_parser_builds_ast_for_pipeline_redirection_and_grouping() {
        let script = parse("{ echo hello > /tmp/out; } | cat").expect("parse script");
        assert_eq!(script.statements.len(), 1);
        let pipeline = &script.statements[0].pipelines[0];
        assert_eq!(pipeline.commands.len(), 2);
        match &pipeline.commands[0] {
            Command::Group(body) => {
                let Command::Simple(simple) = &body[0].pipelines[0].commands[0] else {
                    panic!("expected simple command");
                };
                assert_eq!(simple.redirections.len(), 1);
                assert_eq!(simple.redirections[0].operator, RedirectionOperator::Output);
            }
            _ => panic!("expected group"),
        }
    }

    #[test]
    fn jbc12_syntax_case_statement_matches_upstream_patterns() {
        let cases = [
            (
                r#"case hello in
  hello) echo "matched hello";;
  world) echo "matched world";;
esac"#,
                "matched hello\n",
            ),
            (
                r#"case "anything" in
  specific) echo "specific";;
  *) echo "wildcard";;
esac"#,
                "wildcard\n",
            ),
            (
                r#"case "hello.txt" in
  *.txt) echo "text file";;
  *.md) echo "markdown file";;
  *) echo "other";;
esac"#,
                "text file\n",
            ),
            (
                r#"case "yes" in
  y|yes|Y|YES) echo "confirmed";;
  n|no|N|NO) echo "denied";;
  *) echo "unknown";;
esac"#,
                "confirmed\n",
            ),
            (
                r#"case "test" in
  test) echo "first";;
  test) echo "second";;
  *) echo "wildcard";;
esac"#,
                "first\n",
            ),
            (
                r#"case "nomatch" in
  a) echo "a";;
  b) echo "b";;
esac"#,
                "",
            ),
            (r#"case "x" in x) echo "X";; y) echo "Y";; esac"#, "X\n"),
            (
                r#"case "abc" in
  a?c) echo "matches";;
  *) echo "no match";;
esac"#,
                "matches\n",
            ),
            (
                r#"case "b" in
  [abc]) echo "a, b, or c";;
  [xyz]) echo "x, y, or z";;
  *) echo "other";;
esac"#,
                "a, b, or c\n",
            ),
            (
                r#"case "myfile.bak" in
  *.bak) echo "backup file";;
  *) echo "regular file";;
esac"#,
                "backup file\n",
            ),
            (
                r#"case "multi" in
  multi)
    echo "first"
    echo "second"
    ;;
  *) echo "default";;
esac"#,
                "first\nsecond\n",
            ),
            (
                r#"case $(echo test) in
  test) echo "matched";;
  *) echo "no match";;
esac"#,
                "matched\n",
            ),
            (
                r#"case "default" in
  a) echo "a";;
  *) echo "fallback"
esac"#,
                "fallback\n",
            ),
            (
                r#"case "42" in
  [0-9]) echo "single digit";;
  [0-9][0-9]) echo "double digit";;
  *) echo "other";;
esac"#,
                "double digit\n",
            ),
            (
                r#"case "test" in
  (test) echo "with paren";;
  other) echo "no match";;
esac"#,
                "with paren\n",
            ),
        ];

        for (source, expected_stdout) in cases {
            let result = shell().exec(source);
            assert_eq!(result.stderr, "", "{source}");
            assert_eq!(result.stdout, expected_stdout, "{source}");
            assert_eq!(result.exit_code, 0, "{source}");
        }

        let result = shell()
            .with_env([("FRUIT", "apple")])
            .exec(r#"case $FRUIT in apple) echo "It's an apple";; orange) echo orange;; esac"#);
        assert_eq!(result.stdout, "It's an apple\n");
    }

    #[test]
    fn jbc12_syntax_command_substitution_and_arithmetic_rows_match_upstream() {
        let simple_cases = [
            ("echo $(echo hello)", "hello\n"),
            ("X=$(echo world); echo $X", "world\n"),
            (
                "echo prefix-$(echo middle)-suffix",
                "prefix-middle-suffix\n",
            ),
            ("echo $(echo $(echo nested))", "nested\n"),
            ("COUNT=$(echo 42); echo $COUNT", "42\n"),
            ("echo prefix$(echo)suffix", "prefixsuffix\n"),
            ("echo $((1 + 2))", "3\n"),
            ("echo $((10 - 3))", "7\n"),
            ("echo $((4 * 5))", "20\n"),
            ("echo $((10 / 3))", "3\n"),
            ("echo $((10 % 3))", "1\n"),
            ("echo $((2 ** 8))", "256\n"),
            ("echo $(((2 + 3) * 4))", "20\n"),
            ("echo $((-5 + 3))", "-2\n"),
            ("echo $((2 + 3 * 4 - 1))", "13\n"),
            ("SUM=$((10 + 20)); echo $SUM", "30\n"),
            ("N=5; N=$((N + 1)); echo $N", "6\n"),
        ];

        for (source, expected_stdout) in simple_cases {
            let result = shell().exec(source);
            assert_eq!(result.stderr, "", "{source}");
            assert_eq!(result.stdout, expected_stdout, "{source}");
        }

        let mut file_shell = shell().with_files(ShellVirtualFileSystem::with_files([
            ("/test.txt", "hello\nworld\n"),
            ("/lines.txt", "a\nb\nc\n"),
        ]));
        assert_eq!(
            file_shell.exec("echo $(cat /test.txt | grep world)").stdout,
            "world\n"
        );
        assert_eq!(
            file_shell.exec("echo lines: $(wc -l < /lines.txt)").stdout,
            "lines: 3\n"
        );

        assert_eq!(
            shell()
                .with_files(ShellVirtualFileSystem::with_files([(
                    "/test.txt",
                    "file content"
                )]))
                .exec("echo $(cat /test.txt)")
                .stdout,
            "file content\n"
        );
        assert_eq!(
            shell()
                .with_files(ShellVirtualFileSystem::with_files([(
                    "/test.txt",
                    "line1\nline2\nline3"
                )]))
                .exec("echo $(cat /test.txt)")
                .stdout,
            "line1 line2 line3\n"
        );

        let comparison = shell().exec("echo $((5 > 3)) $((5 < 3)) $((5 == 5)) $((5 != 5))");
        assert_eq!(comparison.stdout, "1 0 1 0\n");
        let logical = shell().exec("echo $((1 && 1)) $((1 && 0)) $((0 || 1)) $((0 || 0))");
        assert_eq!(logical.stdout, "1 0 1 0\n");
        let bitwise = shell().exec("echo $((5 & 3)) $((5 | 3)) $((5 ^ 3))");
        assert_eq!(bitwise.stdout, "1 7 6\n");
        let shifts = shell().exec("echo $((1 << 4)) $((16 >> 2))");
        assert_eq!(shifts.stdout, "16 4\n");
        let mut with_env = shell().with_env([("X", "5")]);
        assert_eq!(with_env.exec("echo $(($X + 3)) $((X + 3))").stdout, "8 8\n");
    }

    #[test]
    fn jbc12_transform_serialize_round_trips_core_ast_rows() {
        fn assert_round_trip(source: &str) {
            let script = parse(source).unwrap_or_else(|error| panic!("{source}: {error}"));
            let serialized = serialize(&script);
            let reparsed = parse(&serialized)
                .unwrap_or_else(|error| panic!("{source} -> {serialized}: {error}"));
            assert_eq!(reparsed, script, "{source} -> {serialized}");
        }

        for source in [
            "echo hello",
            "ls -la /tmp",
            "x=1",
            "VAR=value echo test",
            "PATH+=/new",
            "arr=(a b c)",
            "x=",
            "echo hello | cat",
            "cat file | grep foo | wc -l",
            "! grep foo file",
            "cmd1 |& cmd2",
            "cmd1 && cmd2",
            "cmd1 || cmd2",
            "cmd1; cmd2",
            "cmd1 && cmd2 || cmd3",
            "sleep 10 &",
            "echo hi > file.txt",
            "echo hi >> file.txt",
            "cat < file.txt",
            "cmd 2> err.log",
            "cmd &> all.log",
            "cmd &>> all.log",
            "cmd 2>&1",
            "cmd 2>&-",
            "cat <<< hello",
            "echo hi >| file.txt",
            "cmd <> file.txt",
            "echo 'hello world'",
            r#"echo "hello world""#,
            "echo hello\\ world",
            "echo $HOME",
            "echo ${HOME}",
            "echo $(pwd)",
            "echo `pwd`",
            "echo $((1 + 2))",
            "cd ~",
            "ls ~root",
            "ls *.txt",
            "echo {a,b,c}",
            "echo {1..10}",
            "echo {1..10..2}",
            "echo ${#var}",
            "echo ${var:-default}",
            "echo ${var-default}",
            "echo ${var:=default}",
            "echo ${var=default}",
            "echo ${var:+alt}",
            "echo ${var+alt}",
            "echo $?",
            "echo $#",
            "echo $@",
            "echo ${10}",
            "if true; then echo yes; fi",
            "if true; then echo yes; else echo no; fi",
            "if cmd1; then echo 1; elif cmd2; then echo 2; else echo 3; fi",
            "for i in 1 2 3; do echo $i; done",
            "for x; do echo $x; done",
            "while true; do echo loop; done",
            "until false; do echo loop; done",
            "case $x in a) echo a;; b|c) echo bc;; *) echo other;; esac",
            "(echo sub)",
            "{ echo group; }",
            "myfunc() { echo hello; }",
        ] {
            assert_round_trip(source);
        }
    }

    /// Closes the just-bash-core `serialize.test.ts` round-trip rows for
    /// c-style/fallthrough/empty compound commands, compound-with-redirection
    /// forms, arithmetic commands, and nested-substitution / group-in-pipeline
    /// complex scripts. Each source is parsed, serialized, and reparsed; the
    /// reparsed AST must equal the original, so a serializer regression on any
    /// of these node shapes fails the assertion.
    #[test]
    fn just_bash_core_serialize_round_trips_arithmetic_and_compound_redirection_rows() {
        fn assert_round_trip(source: &str) {
            let script = parse(source).unwrap_or_else(|error| panic!("{source}: {error}"));
            let serialized = serialize(&script);
            let reparsed = parse(&serialized)
                .unwrap_or_else(|error| panic!("{source} -> {serialized}: {error}"));
            assert_eq!(reparsed, script, "{source} -> {serialized}");
        }

        for source in [
            // compound commands: c-style for, fallthrough/empty case
            // (serialize.test.ts:151,159,161)
            "for ((i=0; i<10; i++)); do echo $i; done",
            "case $x in a) echo a;& b) echo b;; esac",
            "case $x in a) ;; esac",
            // compound commands with redirections
            // (serialize.test.ts:168,170,172,174)
            "if true; then echo yes; fi > out.txt",
            "for i in 1 2 3; do echo $i; done > out.txt",
            "while true; do echo loop; done > out.txt",
            "case $x in a) echo a;; esac > out.txt",
            // arithmetic command (serialize.test.ts:181..190,192,194,195)
            "((x = 1 + 2))",
            "((x > 5))",
            "((x = a > b ? a : b))",
            "((x++))",
            "((--x))",
            "(((x + y) * z))",
            "((arr[0] + arr[1]))",
            "((arr[0] = 5))",
            "((assoc[key] + 1))",
            "echo $((1 + $((2 + 3))))",
            "echo $((1 + $(echo 2)))",
            "echo $(( ${base}#ff ))",
            "echo $(( ${zero}11 ))",
            // complex scripts: nested command sub, group in pipeline
            // (serialize.test.ts:424,427)
            "echo $(echo $(echo hi))",
            "{ echo a; echo b; } | cat",
        ] {
            assert_round_trip(source);
        }
    }

    #[test]
    fn just_bash_core_serialize_round_trips_param_op_and_compound_rows() {
        fn assert_round_trip(source: &str) {
            let script = parse(source).unwrap_or_else(|error| panic!("{source}: {error}"));
            let serialized = serialize(&script);
            let reparsed = parse(&serialized)
                .unwrap_or_else(|error| panic!("{source} -> {serialized}: {error}"));
            assert_eq!(reparsed, script, "{source} -> {serialized}");
        }

        for source in [
            // pipelines: timed pipelines (serialize.test.ts:53,54)
            "time sleep 1",
            "time -p sleep 1",
            // parameter operations: error-if-unset variants (serialize.test.ts:103,105,106)
            "echo ${var:?error msg}",
            "echo ${var:?}",
            "echo ${var?error}",
            // substring (serialize.test.ts:109,110)
            "echo ${var:2}",
            "echo ${var:0:5}",
            // prefix/suffix removal (serialize.test.ts:111,112,113,114)
            "echo ${var#pattern}",
            "echo ${var##pattern}",
            "echo ${var%pattern}",
            "echo ${var%%pattern}",
            // pattern replacement (serialize.test.ts:115,116,117,119,121)
            "echo ${var/old/new}",
            "echo ${var//old/new}",
            "echo ${var/#old/new}",
            "echo ${var/%old/new}",
            "echo ${var/old}",
            // case modification (serialize.test.ts:123,124,125,126,127)
            "echo ${var^}",
            "echo ${var^^}",
            "echo ${var,}",
            "echo ${var,,}",
            "echo ${var^^[a-z]}",
            // indirection and prefix listing (serialize.test.ts:128,129,130,131,132,133)
            "echo ${!ref}",
            "echo ${!var##pattern}",
            "echo ${!arr[@]}",
            "echo ${!arr[*]}",
            "echo ${!MY@}",
            "echo ${!MY*}",
            // transform op (serialize.test.ts:134)
            "echo ${var@Q}",
            // compound: subshell/group with redirections (serialize.test.ts:176,177)
            "(echo sub) > out.txt",
            "{ echo group; } > out.txt",
        ] {
            assert_round_trip(source);
        }
    }

    /// Closes the just-bash-core `serialize.test.ts` round-trip rows for the
    /// remaining arithmetic-command edge cases, the `[[ ... ]]` conditional
    /// command grammar, and the complex-script rows (pipeline-with-redirections,
    /// function bodies containing compound commands, and pipeline loop bodies).
    /// Each source is parsed, serialized, and reparsed; the reparsed AST must be
    /// structurally identical to the original, so a serializer regression on
    /// single-quoted arithmetic operands, `$`-prefixed arithmetic variables,
    /// compound arithmetic assignment, arithmetic-command redirections, any
    /// `[[ ... ]]` test operator/grouping/regex node, or the nested
    /// compound/pipeline complex-script shapes fails the assertion.
    #[test]
    fn just_bash_core_serialize_round_trips_conditional_and_complex_script_rows() {
        fn assert_round_trip(source: &str) {
            let script = parse(source).unwrap_or_else(|error| panic!("{source}: {error}"));
            let serialized = serialize(&script);
            let reparsed = parse(&serialized)
                .unwrap_or_else(|error| panic!("{source} -> {serialized}: {error}"));
            assert_eq!(reparsed, script, "{source} -> {serialized}");
        }

        for source in [
            // arithmetic command edge rows (serialize.test.ts:196,197,198,200)
            "(( '1' ))",
            "echo $(($x + 1))",
            "((x += 5))",
            "((x = 1)) > /dev/null",
            // conditional command `[[ ... ]]` grammar (serialize.test.ts:204..211)
            "[[ $a == \"foo\" ]]",
            "[[ -f file.txt ]]",
            "[[ -f a && -f b ]]",
            "[[ -f a || -f b ]]",
            "[[ ! -f a ]]",
            "[[ ( -f a || -f b ) && -d c ]]",
            "[[ $x =~ ^[0-9]+$ ]]",
            "[[ \"nonempty\" ]]",
            // complex scripts (serialize.test.ts:421,429,432,435)
            "cmd1 2>&1 | cmd2 > out.txt",
            "f() { if true; then echo yes; fi; }",
            "for i in 1 2 3; do echo $i | cat; done",
            "f() { echo hello; } > out.txt",
        ] {
            assert_round_trip(source);
        }
    }

    /// Closes the just-bash-core `serialize.test.ts` execution-equivalence rows
    /// and the matching `tee-plugin.test.ts` `assertSameSemantics` rows for
    /// scripts the portable Rust interpreter reproduces with the deterministic
    /// command fakes (echo / cat / printf / sort / grep / test). Upstream runs
    /// `parse -> serialize` and asserts the serialized program executes with
    /// byte-identical stdout/stderr/exitCode to the original (catching escaping
    /// edge cases that a pure AST round-trip cannot). The Rust equivalent runs
    /// the original through the real interpreter, serializes the parsed AST, and
    /// re-runs the serialized form; the two executions must match exactly, and
    /// each case also asserts the upstream-correct output so the test fails on
    /// either a serializer or an interpreter regression.
    #[test]
    fn just_bash_core_serialize_and_tee_execution_equivalence_rows() {
        // (source, expected stdout, expected stderr, expected exit code).
        let cases = [
            // serialize.test.ts:299 nested command sub with inner quotes
            ("echo \"$(echo \"$(echo deep)\")\"", "deep\n", "", 0),
            // serialize.test.ts:362 multiple statements
            ("echo first; echo second", "first\nsecond\n", "", 0),
            // serialize.test.ts:363 conditional and logic
            ("true && echo yes || echo no", "yes\n", "", 0),
            // serialize.test.ts:367 case statement
            (
                "x=hello; case $x in hello) echo matched;; *) echo nope;; esac",
                "matched\n",
                "",
                0,
            ),
            // serialize.test.ts:396 array in subshell
            ("(arr=(x y z); echo ${arr[1]})", "y\n", "", 0),
            // tee-plugin.test.ts:451 function definition and call with pipe
            (
                "greet() { echo \"hello $1\"; }; greet world | cat",
                "hello world\n",
                "",
                0,
            ),
            // tee-plugin.test.ts:509 heredoc into pipeline
            (
                "cat <<EOF | sort\nbanana\napple\ncherry\nEOF",
                "apple\nbanana\ncherry\n",
                "",
                0,
            ),
            // tee-plugin.test.ts:515 deeply nested command substitution
            ("echo $(echo $(echo $(echo deep)))", "deep\n", "", 0),
            // tee-plugin.test.ts:641 word splitting (quoted vs unquoted)
            (
                "X=\"a   b   c\"; echo $X; echo \"$X\"",
                "a b c\na   b   c\n",
                "",
                0,
            ),
            // tee-plugin.test.ts:649 multiple here-docs in sequence
            (
                "cat <<A\nfirst\nA\ncat <<B\nsecond\nB",
                "first\nsecond\n",
                "",
                0,
            ),
        ];

        for (source, stdout, stderr, exit_code) in cases {
            // 1. The interpreter must reproduce the upstream-correct semantics.
            let plain = shell().exec(source);
            assert_eq!(plain.stdout, stdout, "plain stdout for: {source}");
            assert_eq!(plain.stderr, stderr, "plain stderr for: {source}");
            assert_eq!(plain.exit_code, exit_code, "plain exit for: {source}");

            // 2. parse -> serialize must be execution-faithful (the upstream
            //    `execEquiv` / `assertSameSemantics` contract).
            let script = parse(source).unwrap_or_else(|error| panic!("{source}: {error}"));
            let serialized = serialize(&script);
            let round = shell().exec(&serialized);
            assert_eq!(
                round.stdout, plain.stdout,
                "round stdout: {source} -> {serialized}"
            );
            assert_eq!(
                round.stderr, plain.stderr,
                "round stderr: {source} -> {serialized}"
            );
            assert_eq!(
                round.exit_code, plain.exit_code,
                "round exit: {source} -> {serialized}"
            );
        }
    }

    /// Closes the remaining just-bash-core `serialize.test.ts` "string escaping
    /// edge cases" execution-equivalence rows (the `execEquiv` describe block).
    /// Upstream runs `parse -> serialize` and asserts the serialized program
    /// executes with byte-identical stdout/stderr/exitCode to the original; this
    /// catches escaping bugs (single-quote metacharacter literals, escaped
    /// special chars, `${...}` parameter ops inside the serializer, mixed
    /// quoting, command substitution with embedded pipes/quotes, brace/tilde/
    /// arithmetic expansions, compound `if`/`for`/`while` bodies, escaped
    /// backslash-before-dollar sequences, dollar in single-then-double quotes,
    /// empty/nested command substitution, and escaped-newline continuations)
    /// that a pure AST round-trip cannot. The Rust equivalent runs each source
    /// through the interpreter, serializes the parsed AST, and re-runs the
    /// serialized form; the two executions must match exactly. It fails on any
    /// serializer escaping regression, since a mis-serialized word would change
    /// the re-executed output.
    #[test]
    fn just_bash_core_serialize_string_escaping_execution_equivalence_rows() {
        let cases = [
            // serialize.test.ts:309 all metacharacters in single quotes
            "echo '|&;<>()$`\\\"!#~*?[]{}'",
            // serialize.test.ts:314 escaped special chars
            "echo a\\&b\\|c\\;d",
            // serialize.test.ts:319 default with special chars
            "echo ${x:-\"hello world\"}",
            // serialize.test.ts:321 nested expansion in default
            "x=greeting; echo ${x:-$(echo fallback)}",
            // serialize.test.ts:323 substring of variable
            "x=hello; echo \"${x:1:3}\"",
            // serialize.test.ts:324 length of variable
            "x=hello; echo ${#x}",
            // serialize.test.ts:325 pattern replacement
            "x=\"hello world\"; echo ${x/world/earth}",
            // serialize.test.ts:327 case modification
            "x=hello; echo ${x^}; echo ${x^^}",
            // serialize.test.ts:353 mixed quoting in arguments
            "echo 'single' \"double\" plain",
            // serialize.test.ts:355 variable assignment then use in quotes
            "x=\"hello world\"; echo \"value: $x\"",
            // serialize.test.ts:357 command sub with pipe in double quotes
            "echo \"lines: $(echo -e \"a\\nb\" | wc -l)\"",
            // serialize.test.ts:359 brace expansion (unquoted)
            "echo {a,b,c}",
            // serialize.test.ts:360 tilde expansion
            "echo ~",
            // serialize.test.ts:361 arithmetic expansion
            "echo $((3 * 7 + 1))",
            // serialize.test.ts:364 if statement
            "if [ 1 -eq 1 ]; then echo \"equal\"; fi",
            // serialize.test.ts:366 for loop
            "for i in a b c; do echo \"item: $i\"; done",
            // serialize.test.ts:380 escaped backslash before dollar
            "x=val; echo \"\\\\$x\"",
            // serialize.test.ts:382 escaped backslash before escaped dollar
            "echo \"\\\\\\$x\"",
            // serialize.test.ts:384 dollar in single quotes then double quotes
            "echo '$literal' \"$HOME\"",
            // serialize.test.ts:386 empty command substitution
            "echo \"$(true)\"",
            // serialize.test.ts:390 nested subshell in double quotes
            "echo \"$(echo \"$(echo deep)\")\"",
            // serialize.test.ts:392 escaped newline continues line
            "echo hello\\\nworld",
            // serialize.test.ts:394 while loop with read
            "echo \"a b c\" | while read x y z; do echo \"$x:$y:$z\"; done",
        ];

        for source in cases {
            let plain = shell().exec(source);
            let script = parse(source).unwrap_or_else(|error| panic!("{source}: {error}"));
            let serialized = serialize(&script);
            let round = shell().exec(&serialized);
            assert_eq!(
                round.stdout, plain.stdout,
                "round stdout: {source} -> {serialized}"
            );
            assert_eq!(
                round.stderr, plain.stderr,
                "round stderr: {source} -> {serialized}"
            );
            assert_eq!(
                round.exit_code, plain.exit_code,
                "round exit: {source} -> {serialized}"
            );
        }

        // Anchor a few cases to their exact upstream-correct output so the row
        // also fails if the interpreter's escaping/expansion regresses, not only
        // the serializer.
        assert_eq!(
            shell().exec("echo a\\&b\\|c\\;d").stdout,
            "a&b|c;d\n",
            "escaped special chars produce literal metacharacters"
        );
        assert_eq!(
            shell().exec("echo '|&;<>()$`\\\"!#~*?[]{}'").stdout,
            "|&;<>()$`\\\"!#~*?[]{}\n",
            "single-quoted metacharacters stay literal"
        );
        assert_eq!(
            shell().exec("x=hello; echo ${#x}").stdout,
            "5\n",
            "length expansion counts characters"
        );
        assert_eq!(
            shell().exec("x=val; echo \"\\\\$x\"").stdout,
            "\\val\n",
            "escaped backslash before dollar keeps the backslash literal"
        );
        assert_eq!(
            shell().exec("echo \"\\\\\\$x\"").stdout,
            "\\$x\n",
            "escaped backslash before escaped dollar yields literal backslash-dollar"
        );
        assert_eq!(
            shell().exec("echo hello\\\nworld").stdout,
            "hello\nworld\n",
            "escaped newline continues the line and splits words"
        );
        assert_eq!(
            shell().exec("echo \"$(echo \"$(echo deep)\")\"").stdout,
            "deep\n",
            "nested command substitution with inner quotes"
        );
    }

    #[test]
    fn jbc12_transform_command_collector_walks_upstream_ast_shapes() {
        let cases = [
            ("echo hello | cat | wc -l", vec!["cat", "echo", "wc"]),
            (
                r#"if true; then echo "yes"; else echo "no"; fi"#,
                vec!["echo", "true"],
            ),
            ("for i in a b c; do echo $i; done", vec!["echo"]),
            ("echo $(echo inner)", vec!["echo"]),
            (
                "x=a; case $x in a) echo matched;; b) printf nope;; esac",
                vec!["echo", "printf"],
            ),
            ("echo a; echo b; cat /dev/null; echo c", vec!["cat", "echo"]),
            (
                r#"echo "a b c" | while read x y z; do echo "$x"; done"#,
                vec!["echo", "read"],
            ),
            ("myfunc() { echo hello; }", vec!["echo"]),
        ];

        for (source, expected) in cases {
            let script = parse(source).unwrap_or_else(|error| panic!("{source}: {error}"));
            assert_eq!(collect_command_names(&script), expected, "{source}");
        }

        let source = r#"x=5; echo $((x * 2)); echo "done""#;
        let script = parse(source).expect("parse collector no-op script");
        let before = serialize(&script);
        assert_eq!(collect_command_names(&script), vec!["echo"]);
        assert_eq!(serialize(&script), before);

        let plain = shell().exec(source);
        let after_collect = shell().exec(&before);
        assert_eq!(after_collect.stdout, plain.stdout);
        assert_eq!(after_collect.stderr, plain.stderr);
        assert_eq!(after_collect.exit_code, plain.exit_code);
    }

    #[test]
    fn jbc19_shell_join_args_quotes_and_preserves_literal_arguments() {
        assert_eq!(shell_join_args(["echo", "hello"]), "'echo' 'hello'");
        assert_eq!(shell_join_args(Vec::<String>::new()), "");
        assert_eq!(shell_join_args(["ls"]), "'ls'");
        assert_eq!(
            shell_join_args(["echo", "hello world"]),
            "'echo' 'hello world'"
        );
        assert_eq!(shell_join_args(["echo", "it's"]), "'echo' 'it'\\''s'");
        assert_eq!(shell_join_args(["echo", ""]), "'echo' ''");
        assert_eq!(
            shell_join_args([
                "echo",
                "$(whoami)",
                "; rm -rf /",
                "`id`",
                "a|b",
                "a&b",
                "a>b"
            ]),
            "'echo' '$(whoami)' '; rm -rf /' '`id`' 'a|b' 'a&b' 'a>b'"
        );
        assert_eq!(
            shell_join_args(["echo", "line1\nline2", "col1\tcol2"]),
            "'echo' 'line1\nline2' 'col1\tcol2'"
        );

        assert_eq!(
            shell()
                .exec(&shell_join_args(["echo", "$(echo INJECTED)"]))
                .stdout,
            "$(echo INJECTED)\n"
        );
        assert_eq!(
            shell()
                .exec(&shell_join_args(["echo", "it's a test"]))
                .stdout,
            "it's a test\n"
        );
        assert_eq!(
            shell()
                .exec(&shell_join_args(["echo", "safe; echo INJECTED"]))
                .stdout,
            "safe; echo INJECTED\n"
        );
        assert_eq!(
            shell()
                .exec(&shell_join_args(["printf", "%s|", "a", "", "b"]))
                .stdout,
            "a||b|"
        );
        assert_eq!(
            shell()
                .exec(&shell_join_args(["echo", "hello   world"]))
                .stdout,
            "hello   world\n"
        );
    }

    #[test]
    fn jbc19_heredoc_rows_parse_serialize_and_execute_with_expansion() {
        fn assert_round_trip(source: &str) {
            let script = parse(source).unwrap_or_else(|error| panic!("{source}: {error}"));
            let serialized = serialize(&script);
            let reparsed = parse(&serialized)
                .unwrap_or_else(|error| panic!("{source} -> {serialized}: {error}"));
            assert_eq!(reparsed, script, "{source} -> {serialized}");
        }

        fn assert_exec_equiv(source: &str) {
            let script = parse(source).unwrap_or_else(|error| panic!("{source}: {error}"));
            let serialized = serialize(&script);
            let mut original = shell().with_env([("x", "world")]);
            let mut transformed = shell().with_env([("x", "world")]);
            let original = original.exec(source);
            let transformed = transformed.exec(&serialized);
            assert_eq!(
                (
                    transformed.stdout,
                    transformed.stderr,
                    transformed.exit_code
                ),
                (original.stdout, original.stderr, original.exit_code),
                "{source} -> {serialized}"
            );
        }

        for source in [
            "cat <<EOF\nhello\nEOF",
            "cat <<'EOF'\nhello $name\nEOF",
            "cat <<\"EOF\"\nhello $name\nEOF",
            "cat <<-EOF\n\thello\n\tworld\nEOF",
            "cat <<EOF\nline1\nline2\nline3\nEOF",
            "cat <<EOF\n\nline\n\nEOF",
            "cat <<EOF\nresult: $(echo hi)\nEOF",
            "grep pattern <<EOF\nfoo pattern bar\nEOF",
            "cat <<EOF | sort\nbanana\napple\ncherry\nEOF",
            "<<EOF cat\nprefix heredoc\nEOF",
            "cat <<END-TEST\n  content with spaces\nEND-TEST",
        ] {
            assert_round_trip(source);
        }

        for source in [
            "x=world; cat <<EOF\nhello $x\nEOF",
            "cat <<'EOF'\nhello $x\nEOF",
            "cat <<EOF\nresult: $(echo 42)\nEOF",
            "cat <<EOF\n!@#$%^&*()\nEOF",
            "cat <<EOF\n!@#$%^&*\nEOF",
            "cat <<EOF\nresult: `echo hi`\nEOF",
            "cat <<EOF\n\nline\n\nEOF",
            "cat <<EOF\n\ttabbed\nEOF",
            "cat <<'EOF'\n`not a command`\nEOF",
            "cat <<EOF\nprice: \\$5\nEOF",
            "cat <<-EOF\n\thello\n\tworld\nEOF",
            "cat <<EOF\nline1\\\nline2\nEOF",
        ] {
            assert_exec_equiv(source);
        }

        let mut env_shell = shell().with_env([("NAME", "Alice"), ("x", "world")]);
        assert_eq!(
            env_shell.exec("cat <<EOF\nHello, $NAME!\nEOF").stdout,
            "Hello, Alice!\n"
        );
        assert_eq!(
            env_shell.exec("cat <<'EOF'\nHello, $NAME!\nEOF").stdout,
            "Hello, $NAME!\n"
        );
        assert_eq!(
            env_shell.exec("cat <<\"EOF\"\nHello, $NAME!\nEOF").stdout,
            "Hello, $NAME!\n"
        );
        assert_eq!(
            env_shell
                .exec("cat <<EOF\nresult: $(echo 42)\nresult: `echo hi`\nprice: \\$5\nEOF")
                .stdout,
            "result: 42\nresult: hi\nprice: $5\n"
        );
        assert_eq!(
            shell().exec("cat <<-EOF\n\thello\n\tworld\nEOF").stdout,
            "hello\nworld\n"
        );
        assert_eq!(
            shell()
                .exec("cat <<EOF | grep hello\nhello world\ngoodbye world\nEOF")
                .stdout,
            "hello world\n"
        );
        assert_eq!(
            shell()
                .exec("cat <<EOF | sort\nbanana\napple\ncherry\nEOF")
                .stdout,
            "apple\nbanana\ncherry\n"
        );
        assert_eq!(
            shell().exec(r#"cat <<< "hello world""#).stdout,
            "hello world\n"
        );
        assert_eq!(
            shell()
                .with_env([("x", "hello")])
                .exec(r#"cat <<< "$x world""#)
                .stdout,
            "hello world\n"
        );
        assert_eq!(
            shell().exec("wc -l <<EOF\none\ntwo\nthree\nEOF").stdout,
            "3\n"
        );
        assert_eq!(
            shell()
                .exec("grep world <<EOF\nhello world\ngoodbye world\njust hello\nEOF")
                .stdout,
            "hello world\ngoodbye world\n"
        );
        assert_eq!(shell().exec("cat <<EOF\nEOF").stdout, "");
        assert_eq!(shell().exec("cat <<EOF\n\nEOF").stdout, "\n");
        let multiple_commands = shell().exec("cat <<EOF\nhello\nEOF\necho done");
        assert_eq!(
            (
                multiple_commands.stdout,
                multiple_commands.stderr,
                multiple_commands.exit_code
            ),
            ("hello\ndone\n".to_string(), String::new(), 0)
        );
        assert_eq!(shell().exec("cat <<EOF\n{a,b}\nEOF").stdout, "{a,b}\n");
        assert_eq!(
            shell().exec("cat <<EOF\nline1\\\nline2\nEOF").stdout,
            "line1\\\nline2\n"
        );
        assert_eq!(
            shell()
                .exec("if [[ 1 -eq 1 ]]; then\ncat <<EOF\nhello from if\nEOF\nfi")
                .stdout,
            "hello from if\n"
        );
        assert_eq!(
            shell().exec("cat <<MYDELIM\ncontent here\nMYDELIM").stdout,
            "content here\n"
        );
        assert_eq!(
            shell()
                .exec("cat <<EOF\n    four spaces\n\ttab\n  EOF\nEOF")
                .stdout,
            "    four spaces\n\ttab\n  EOF\n"
        );
        assert_eq!(
            shell()
                .exec("NAME=World; cat <<EOF\nHello, $NAME!\nEOF")
                .stdout,
            "Hello, World!\n"
        );
    }

    #[test]
    fn jbc19_transform_serialize_quoting_edge_rows_round_trip() {
        fn assert_round_trip(source: &str) {
            let script = parse(source).unwrap_or_else(|error| panic!("{source}: {error}"));
            let serialized = serialize(&script);
            let reparsed = parse(&serialized)
                .unwrap_or_else(|error| panic!("{source} -> {serialized}: {error}"));
            assert_eq!(reparsed, script, "{source} -> {serialized}");
        }

        for source in [
            "echo hello\\ world",
            "echo 'a&b|c;d'",
            r#"echo "hello $name""#,
            r#"echo "result: `echo hi`""#,
            r#"echo "result: $(echo hi)""#,
            r#"echo "it's fine""#,
            "echo '\"quoted\"'",
            "echo ''",
            r#"echo """#,
            "echo 'a'\"b\"'c'",
            "echo test\\\\",
            "echo hello\\nworld",
            "echo 'a\tb'",
            "echo 'file*.txt'",
            "echo '(test)'",
            "echo '[test]'",
            "echo '#comment'",
            "echo '!bang'",
            "echo '~user'",
            "echo '{a,b}'",
            r#"echo "${var}""#,
            r#"echo "$(echo "inner")""#,
            r#"echo "total: $((1 + 2))""#,
        ] {
            assert_round_trip(source);
        }

        let mut env_shell = shell().with_env([("HOME", "/home/user"), ("x", "val")]);
        for (source, expected_stdout) in [
            ("echo 'hello world'", "hello world\n"),
            (r#"echo "hello world""#, "hello world\n"),
            ("echo ''", "\n"),
            (r#"echo """#, "\n"),
            ("echo 'a'\"b\"'c'", "abc\n"),
            (r#"echo "price is \$5""#, "price is $5\n"),
            (r#"echo "say \"hello\"""#, "say \"hello\"\n"),
            (r#"echo "\`not a command\`""#, "`not a command`\n"),
            (r#"echo "path\\dir""#, "path\\dir\n"),
            (r#"echo "line1\nline2""#, "line1\\nline2\n"),
            (r#"echo "home: $HOME""#, "home: /home/user\n"),
            (r#"echo "home: ${HOME}""#, "home: /home/user\n"),
            (r#"echo "result: $(echo hi)""#, "result: hi\n"),
            (r#"echo "result: `echo hi`""#, "result: hi\n"),
            (r#"echo "total: $((2 + 3))""#, "total: 5\n"),
            ("echo '$HOME'", "$HOME\n"),
            ("echo '`cmd`'", "`cmd`\n"),
            ("echo '\"quoted\"'", "\"quoted\"\n"),
            ("echo 'back\\slash'", "back\\slash\n"),
            ("echo '!bang'", "!bang\n"),
            ("echo '#not a comment'", "#not a comment\n"),
            ("echo hello\\ world", "hello world\n"),
            ("echo \\*.txt", "*.txt\n"),
            ("echo \\#not-comment", "#not-comment\n"),
            (r#"echo "*.txt""#, "*.txt\n"),
            (r#"echo "\$x is $x""#, "$x is val\n"),
            (r#"echo "end\$""#, "end$\n"),
            (r#"echo "\$start""#, "$start\n"),
            (r#"echo "\$a \$b \$c""#, "$a $b $c\n"),
        ] {
            let result = env_shell.exec(source);
            assert_eq!(result.stderr, "", "{source}");
            assert_eq!(result.stdout, expected_stdout, "{source}");
        }
    }

    #[test]
    fn jbc19_pipeline_stderr_rows_keep_regular_and_pipe_stderr_separate() {
        let mut interp = shell();

        let regular = interp.exec("err out parent_err | cat");
        assert_eq!(regular.stdout, "out\n");
        assert_eq!(regular.stderr, "parent_err\n");

        let pipe_stderr = interp.exec("err out piped_err |& cat");
        assert_eq!(pipe_stderr.stdout, "out\npiped_err\n");
        assert_eq!(pipe_stderr.stderr, "");

        let first_of_three = interp.exec("err first first_err | cat | cat");
        assert_eq!(first_of_three.stdout, "first\n");
        assert_eq!(first_of_three.stderr, "first_err\n");

        let middle = interp.exec("echo hello | err middle middle_err | cat");
        assert_eq!(middle.stdout, "middle\n");
        assert_eq!(middle.stderr, "middle_err\n");

        let last = interp.exec("echo hello | err last last_err 7");
        assert_eq!(last.stdout, "last\n");
        assert_eq!(last.stderr, "last_err\n");
        assert_eq!(last.exit_code, 7);

        let exit_code = interp.exec("echo hello | grep nomatch");
        assert_eq!(exit_code.exit_code, 1);
    }

    #[test]
    fn jbc19_transform_tee_plugin_metadata_and_script_rows() {
        let fixed_ts = "2024-01-15T10:30:45.123Z";
        let sanitized_ts = "2024-01-15T10-30-45.123Z";

        let mut no_plugins = BashTransformPipeline::new();
        let unchanged = no_plugins.transform("echo hello | cat").unwrap();
        assert_eq!(unchanged.script, "echo hello | cat");
        assert!(unchanged.metadata.is_empty());

        let mut single =
            BashTransformPipeline::new().use_plugin(TeePlugin::new("/tmp/logs", fixed_ts));
        let single_result = single.transform("echo hello").unwrap();
        assert_eq!(single_result.script, "echo hello");
        assert!(single_result.metadata.tee_files.is_empty());

        let mut tee =
            BashTransformPipeline::new().use_plugin(TeePlugin::new("/tmp/logs", fixed_ts));
        let result = tee.transform("echo hello | grep hello").unwrap();
        assert!(
            result
                .script
                .contains(&format!("tee /tmp/logs/{sanitized_ts}-000-echo.stdout.txt"))
        );
        assert!(
            result
                .script
                .contains(&format!("tee /tmp/logs/{sanitized_ts}-001-grep.stdout.txt"))
        );
        assert!(result.script.contains("__tps0=${PIPESTATUS[0]}"));
        assert!(result.script.contains("(exit $__tps0) | (exit $__tps1)"));
        assert_eq!(
            result.metadata.tee_files,
            vec![
                TeeFileInfo {
                    command_index: 0,
                    command_name: "echo".to_string(),
                    command: "echo hello".to_string(),
                    stdout_file: format!("/tmp/logs/{sanitized_ts}-000-echo.stdout.txt"),
                },
                TeeFileInfo {
                    command_index: 1,
                    command_name: "grep".to_string(),
                    command: "grep hello".to_string(),
                    stdout_file: format!("/tmp/logs/{sanitized_ts}-001-grep.stdout.txt"),
                },
            ]
        );

        let mut targeted = BashTransformPipeline::new().use_plugin(
            TeePlugin::new("/tmp/logs", fixed_ts)
                .with_target_command_pattern(Regex::new("^grep$").unwrap()),
        );
        let targeted_result = targeted
            .transform("cat file | sort | grep pattern | wc -l")
            .unwrap();
        assert_eq!(targeted_result.metadata.tee_files.len(), 1);
        assert_eq!(targeted_result.metadata.tee_files[0].command_name, "grep");
        assert!(targeted_result.script.contains("sort | grep pattern | tee"));

        let mut dynamic =
            BashTransformPipeline::new().use_plugin(TeePlugin::new("/tmp/logs", fixed_ts));
        let dynamic_result = dynamic.transform("$cmd hello | cat").unwrap();
        assert!(dynamic_result.script.contains("000-unknown.stdout.txt"));

        let mut multi =
            BashTransformPipeline::new().use_plugin(TeePlugin::new("/tmp/logs", fixed_ts));
        let multi_result = multi.transform("echo a | cat\necho b | cat").unwrap();
        assert_eq!(
            multi_result
                .metadata
                .tee_files
                .iter()
                .map(|file| file.command_index)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );
        assert!(!multi_result.script.contains("10:30:45"));
        assert!(multi_result.script.contains("10-30-45"));
    }

    /// Mirrors the upstream `TeePlugin error handling` describe block plus the
    /// `TeePlugin transform output` AST-rewrite rows in
    /// `packages/just-bash/src/transform/plugins/tee-plugin.test.ts` (lines
    /// 711-806). These rows assert the exact transform contract: which commands
    /// are wrapped with `tee`, that single commands / `&&`/`||` chains /
    /// compound constructs are NOT wrapped, that PIPESTATUS is saved and
    /// restored whenever any stage was wrapped, and that the persistent counter
    /// produces monotonically increasing, unique log-file indices. The Rust
    /// transform emits newline statement separators where upstream emits ` ; `,
    /// so the wrapped-script shape is asserted via substring/structure rather
    /// than byte-identical equality (the metadata is asserted exactly). Each
    /// assertion fails if the wrap-selection, PIPESTATUS rewrite, or counter
    /// logic regresses.
    #[test]
    fn jbc19_tee_plugin_error_handling_and_transform_output_rows() {
        let fixed_ts = "2024-01-15T10:30:45.123Z";
        let sanitized_ts = "2024-01-15T10-30-45.123Z";
        let dir = format!("/tmp/logs/{sanitized_ts}");
        let new_tee =
            || BashTransformPipeline::new().use_plugin(TeePlugin::new("/tmp/logs", fixed_ts));

        // line 711 / 743: single command — no wrapping (no existing pipe).
        let mut single = new_tee();
        let single_result = single.transform("echo hello").unwrap();
        assert_eq!(single_result.script, "echo hello");
        assert!(single_result.metadata.tee_files.is_empty());

        // line 749: pipeline wraps each command and saves + restores PIPESTATUS.
        let mut pipeline = new_tee();
        let pipeline_result = pipeline.transform("echo hello | grep hello").unwrap();
        assert!(
            pipeline_result
                .script
                .contains(&format!("tee {dir}-000-echo.stdout.txt"))
        );
        assert!(
            pipeline_result
                .script
                .contains(&format!("tee {dir}-001-grep.stdout.txt"))
        );
        assert!(
            pipeline_result
                .script
                .contains("__tps0=${PIPESTATUS[0]} __tps1=${PIPESTATUS[2]}")
        );
        assert!(
            pipeline_result
                .script
                .contains("(exit $__tps0) | (exit $__tps1)")
        );
        assert_eq!(pipeline_result.metadata.tee_files.len(), 2);

        // line 721: pipeline exit code preserved through PIPESTATUS — the
        // grep-failure pipeline still wraps both stages and records two tee files.
        let mut preserve = new_tee();
        let preserve_result = preserve.transform("echo hello | grep nomatch").unwrap();
        assert_eq!(preserve_result.metadata.tee_files.len(), 2);
        assert!(
            preserve_result
                .script
                .contains("__tps0=${PIPESTATUS[0]} __tps1=${PIPESTATUS[2]}")
        );

        // line 756: single commands in && / || chains — no wrapping.
        let mut chains = new_tee();
        let chains_result = chains
            .transform("echo first && echo second || echo third")
            .unwrap();
        assert_eq!(
            chains_result.script,
            "echo first && echo second || echo third"
        );
        assert!(chains_result.metadata.tee_files.is_empty());

        // line 762: pipeline in && chain wraps the pipeline, skips the single
        // command, and leaves the trailing `&& echo found` intact.
        let mut chain_pipe = new_tee();
        let chain_pipe_result = chain_pipe
            .transform("echo hello | grep hello && echo found")
            .unwrap();
        assert!(
            chain_pipe_result
                .script
                .contains(&format!("tee {dir}-000-echo.stdout.txt"))
        );
        assert!(
            chain_pipe_result
                .script
                .contains(&format!("tee {dir}-001-grep.stdout.txt"))
        );
        assert!(chain_pipe_result.script.contains("&& echo found"));
        assert_eq!(chain_pipe_result.metadata.tee_files.len(), 2);

        // line 770: assignment-only and single commands — no wrapping. Both
        // statements are single-command pipelines, so the rewrite only splits
        // the `;` into a statement separator and records no tee files.
        let mut assign = new_tee();
        let assign_result = assign.transform("VAR=hello; echo $VAR").unwrap();
        assert!(assign_result.metadata.tee_files.is_empty());
        assert!(assign_result.script.contains("VAR=hello"));
        assert!(assign_result.script.contains("echo $VAR"));
        assert!(!assign_result.script.contains("tee "));

        // line 777: compound commands are not wrapped.
        let mut compound = new_tee();
        let compound_result = compound.transform("if true; then echo yes; fi").unwrap();
        assert!(compound_result.metadata.tee_files.is_empty());
        assert!(!compound_result.script.contains("tee "));

        // line 783: persistent counter across pipelines yields unique, ordered
        // indices for every wrapped stage across separate statements.
        let mut counter = new_tee();
        let counter_result = counter.transform("echo a | cat; echo b | cat").unwrap();
        assert!(counter_result.script.contains("000-echo"));
        assert!(counter_result.script.contains("001-cat"));
        assert!(counter_result.script.contains("002-echo"));
        assert!(counter_result.script.contains("003-cat"));
        assert_eq!(
            counter_result
                .metadata
                .tee_files
                .iter()
                .map(|file| file.command_index)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );

        // line 791: still saves/restores PIPESTATUS even when only some commands
        // are wrapped. With targetCommandPattern=^echo$, only `echo` is wrapped
        // (one tee file), but `tee` inflates PIPESTATUS from 2 to 3 entries so
        // the save/restore epilogue is still emitted.
        let mut partial = BashTransformPipeline::new().use_plugin(
            TeePlugin::new("/tmp/logs", fixed_ts)
                .with_target_command_pattern(Regex::new("^echo$").unwrap()),
        );
        let partial_result = partial.transform("echo hello | cat").unwrap();
        assert!(
            partial_result
                .script
                .contains(&format!("tee {dir}-000-echo.stdout.txt"))
        );
        assert!(
            partial_result
                .script
                .contains("__tps0=${PIPESTATUS[0]} __tps1=${PIPESTATUS[2]}")
        );
        assert!(
            partial_result
                .script
                .contains("(exit $__tps0) | (exit $__tps1)")
        );
        assert_eq!(partial_result.metadata.tee_files.len(), 1);
    }

    #[test]
    fn jbc19_tee_plugin_exec_describe_rows() {
        // Mirrors packages/just-bash/src/transform/plugins/tee-plugin.test.ts
        // "TeePlugin exec" describe block. Each block asserts the observable
        // AST-rewrite contract: which commands are wrapped in `tee`, the
        // recorded metadata (commandName/command/stdoutFile), and the counter.
        let fixed_ts = "2024-01-15T10:30:45.123Z";
        let sanitized_ts = "2024-01-15T10-30-45.123Z";

        let new_tee =
            || BashTransformPipeline::new().use_plugin(TeePlugin::new("/tmp/logs", fixed_ts));

        // line 9: does not wrap single commands (no existing pipe).
        let mut single = new_tee();
        let single_result = single.transform("echo hello").unwrap();
        assert_eq!(single_result.script, "echo hello");
        assert!(single_result.metadata.tee_files.is_empty());

        // line 23: captures stdout for each command in pipeline.
        let mut two = new_tee();
        let two_result = two.transform("cat /data/input.txt | grep hello").unwrap();
        assert_eq!(two_result.metadata.tee_files.len(), 2);
        assert_eq!(two_result.metadata.tee_files[0].command_name, "cat");
        assert_eq!(
            two_result.metadata.tee_files[0].command,
            "cat /data/input.txt"
        );
        assert_eq!(two_result.metadata.tee_files[1].command_name, "grep");
        assert_eq!(two_result.metadata.tee_files[1].command, "grep hello");

        // line 48: only captures targeted commands in pipeline.
        let mut echo_only = BashTransformPipeline::new().use_plugin(
            TeePlugin::new("/tmp/logs", fixed_ts)
                .with_target_command_pattern(Regex::new("^echo$").unwrap()),
        );
        let echo_only_result = echo_only.transform("echo hello | cat").unwrap();
        assert_eq!(echo_only_result.metadata.tee_files.len(), 1);
        assert_eq!(echo_only_result.metadata.tee_files[0].command_name, "echo");
        assert_eq!(echo_only_result.metadata.tee_files[0].command, "echo hello");

        // line 69: captures output from pipeline with multiple stages.
        let mut three = new_tee();
        let three_result = three
            .transform("cat /data/words.txt | grep ^a | sort")
            .unwrap();
        assert_eq!(
            three_result
                .metadata
                .tee_files
                .iter()
                .map(|file| file.command_name.as_str())
                .collect::<Vec<_>>(),
            vec!["cat", "grep", "sort"]
        );

        // line 97: writes to nested output directory.
        let mut nested =
            BashTransformPipeline::new().use_plugin(TeePlugin::new("/tmp/logs/deep/dir", fixed_ts));
        let nested_result = nested.transform("echo test | cat").unwrap();
        assert_eq!(nested_result.metadata.tee_files[0].command_name, "echo");
        assert_eq!(
            nested_result.metadata.tee_files[0].stdout_file,
            format!("/tmp/logs/deep/dir/{sanitized_ts}-000-echo.stdout.txt")
        );

        // line 130: preserves pipeline semantics (PIPESTATUS save/restore) so the
        // last command exit code wins even with tee wrapping inserted.
        let mut grep_fail = new_tee();
        let grep_fail_result = grep_fail.transform("echo hello | grep nomatch").unwrap();
        assert_eq!(grep_fail_result.metadata.tee_files.len(), 2);
        assert!(grep_fail_result.script.contains("__tps0=${PIPESTATUS[0]}"));
        assert!(
            grep_fail_result
                .script
                .contains("(exit $__tps0) | (exit $__tps1)")
        );

        // line 161: skips single commands in && and || chains.
        let mut chain = new_tee();
        let chain_result = chain
            .transform("echo first && echo second; false || echo fallback")
            .unwrap();
        assert!(chain_result.metadata.tee_files.is_empty());

        // line 175: skips compound commands (if/for/while/subshell/group).
        let mut compound = new_tee();
        let compound_result = compound.transform("if true; then echo y; fi").unwrap();
        assert!(compound_result.metadata.tee_files.is_empty());

        // line 187: wraps pipelines inside && chains when a pipe exists, but the
        // trailing single command is left untouched.
        let mut chain_pipe = new_tee();
        let chain_pipe_result = chain_pipe
            .transform("cat /data/f.txt | grep hello && echo found")
            .unwrap();
        assert_eq!(chain_pipe_result.metadata.tee_files.len(), 2);
        assert_eq!(chain_pipe_result.metadata.tee_files[0].command_name, "cat");
        assert_eq!(chain_pipe_result.metadata.tee_files[1].command_name, "grep");

        // line 206: returns empty teeFiles when targetCommandPattern matches nothing.
        let mut no_match = BashTransformPipeline::new().use_plugin(
            TeePlugin::new("/tmp/logs", fixed_ts)
                .with_target_command_pattern(Regex::new("^nonexistent_command$").unwrap()),
        );
        let no_match_result = no_match.transform("echo hello | cat").unwrap();
        assert!(no_match_result.metadata.tee_files.is_empty());

        // line 222: targetCommandPattern matches multiple different commands.
        let mut multi_target = BashTransformPipeline::new().use_plugin(
            TeePlugin::new("/tmp/logs", fixed_ts)
                .with_target_command_pattern(Regex::new("^(cat|sort)$").unwrap()),
        );
        let multi_target_result = multi_target
            .transform("cat /data/file.txt | grep hello | sort")
            .unwrap();
        assert_eq!(multi_target_result.metadata.tee_files.len(), 2);
        assert_eq!(
            multi_target_result.metadata.tee_files[0].command_name,
            "cat"
        );
        assert_eq!(
            multi_target_result.metadata.tee_files[1].command_name,
            "sort"
        );

        // line 242: multiple transform calls produce unique file paths via the
        // persistent counter.
        let mut persistent = new_tee();
        let first = persistent.transform("echo first | cat").unwrap();
        let second = persistent.transform("echo second | cat").unwrap();
        assert!(!first.metadata.tee_files.is_empty());
        assert!(!second.metadata.tee_files.is_empty());
        assert_ne!(
            first.metadata.tee_files[0].stdout_file,
            second.metadata.tee_files[0].stdout_file
        );

        // line 262: mixed targeted and non-targeted commands in one pipeline.
        let mut mixed = BashTransformPipeline::new().use_plugin(
            TeePlugin::new("/tmp/logs", fixed_ts)
                .with_target_command_pattern(Regex::new("^cat$").unwrap()),
        );
        let mixed_result = mixed.transform("cat /data/file.txt | wc -l").unwrap();
        assert_eq!(mixed_result.metadata.tee_files.len(), 1);
        assert_eq!(mixed_result.metadata.tee_files[0].command_name, "cat");

        // line 284: compound command (while loop) inside a pipeline is skipped,
        // while the leading simple command is captured exactly once.
        let mut while_pipe = new_tee();
        let while_pipe_result = while_pipe
            .transform("echo -e 'a\\nb\\nc' | while read line; do echo \"got: $line\"; done")
            .unwrap();
        let echo_entries: Vec<&TeeFileInfo> = while_pipe_result
            .metadata
            .tee_files
            .iter()
            .filter(|file| file.command_name == "echo")
            .collect();
        assert_eq!(echo_entries.len(), 1);
        assert_eq!(echo_entries[0].command, "echo -e 'a\\nb\\nc'");
    }

    #[test]
    fn jbc19_transform_plugin_ordering_and_metadata_rows() {
        #[derive(Default)]
        struct CustomPlugin;

        impl TransformPlugin for CustomPlugin {
            fn name(&self) -> &str {
                "custom"
            }

            fn transform(&mut self, context: TransformContext<'_>) -> ShellResult<TransformResult> {
                let mut custom = BTreeMap::new();
                custom.insert("custom".to_string(), true);
                Ok(TransformResult {
                    ast: context.ast.clone(),
                    metadata: TransformMetadata {
                        custom,
                        ..TransformMetadata::default()
                    },
                })
            }
        }

        struct RewritePlugin;

        impl TransformPlugin for RewritePlugin {
            fn name(&self) -> &str {
                "rewrite"
            }

            fn transform(
                &mut self,
                _context: TransformContext<'_>,
            ) -> ShellResult<TransformResult> {
                Ok(TransformResult {
                    ast: parse("echo transformed").expect("rewrite parses"),
                    metadata: TransformMetadata {
                        custom: BTreeMap::from([("rewritten".to_string(), true)]),
                        ..TransformMetadata::default()
                    },
                })
            }
        }

        struct FailingPlugin;

        impl TransformPlugin for FailingPlugin {
            fn name(&self) -> &str {
                "failing"
            }

            fn transform(
                &mut self,
                _context: TransformContext<'_>,
            ) -> ShellResult<TransformResult> {
                Err(ShellError::new("plugin failed", 1, 1))
            }
        }

        let mut tee_then_collector = BashTransformPipeline::new()
            .use_plugin(TeePlugin::new("/tmp/logs", "2024-01-15T10:30:45.123Z"))
            .use_plugin(CommandCollectorPlugin);
        let ordered = tee_then_collector
            .transform("echo hello | grep hello")
            .unwrap();
        assert_eq!(
            ordered.metadata.commands,
            vec!["echo", "exit", "grep", "tee"]
        );
        assert_eq!(ordered.metadata.tee_files.len(), 2);
        assert_eq!(ordered.metadata.tee_files[0].command_name, "echo");
        assert_eq!(ordered.metadata.tee_files[1].command_name, "grep");

        let mut collector = BashTransformPipeline::new().use_plugin(CommandCollectorPlugin);
        assert_eq!(
            collector
                .transform("echo hello | cat")
                .unwrap()
                .metadata
                .commands,
            vec!["cat", "echo"]
        );

        let mut merged = BashTransformPipeline::new()
            .use_plugin(CommandCollectorPlugin)
            .use_plugin(CustomPlugin);
        let merged_result = merged.transform("echo hello").unwrap();
        assert_eq!(merged_result.metadata.commands, vec!["echo"]);
        assert_eq!(merged_result.metadata.custom.get("custom"), Some(&true));

        let mut rewritten = BashTransformPipeline::new().use_plugin(RewritePlugin);
        let rewritten_result = rewritten.transform("echo original").unwrap();
        assert_eq!(rewritten_result.script, "echo transformed");
        assert_eq!(
            rewritten_result.metadata.custom.get("rewritten"),
            Some(&true)
        );

        let mut failing = BashTransformPipeline::new().use_plugin(FailingPlugin);
        let error = failing
            .transform("echo hello")
            .expect_err("plugin should fail");
        assert!(error.to_string().contains("plugin failed"));
    }

    #[test]
    fn jbc33_syntax_variables_operators_and_loop_rows_match_upstream() {
        let mut env_shell = shell().with_env([
            ("NAME", "world"),
            ("PREFIX", "pre"),
            ("A", "hello"),
            ("B", "world"),
            ("HOME", "/home/user"),
            ("SET", "value"),
        ]);
        env_shell
            .files_mut()
            .write("/home/user/file.txt", "content");
        for (source, expected_stdout) in [
            ("echo hello $NAME", "hello world\n"),
            ("echo hello ${NAME}", "hello world\n"),
            ("echo ${PREFIX}fix", "prefix\n"),
            ("echo $A $B", "hello world\n"),
            (r#"echo "[$UNSET]""#, "[]\n"),
            ("echo ${MISSING:-default}", "default\n"),
            ("echo ${SET:-default}", "value\n"),
            (r#"echo "the $NAME is here""#, "the world is here\n"),
            ("echo 'the $NAME is here'", "the $NAME is here\n"),
            ("cat $HOME/file.txt", "content"),
            ("export FOO=bar; echo $FOO", "bar\n"),
            ("export A=1 B=2 C=3; echo $A $B $C", "1 2 3\n"),
            (r#"unset FOO; echo "[$FOO]""#, "[]\n"),
            (r#"echo "hello   world""#, "hello   world\n"),
            ("echo 'hello   world'", "hello   world\n"),
            (r#"echo "it's working""#, "it's working\n"),
            (r#"echo "say \"hello\"""#, "say \"hello\"\n"),
            (r#"echo """#, "\n"),
            ("echo \"hello\"'world'", "helloworld\n"),
            ("echo 'hello $NAME && test'", "hello $NAME && test\n"),
            ("echo -e \"hello\\nworld\"", "hello\nworld\n"),
            ("echo -e \"col1\\tcol2\"", "col1\tcol2\n"),
            ("echo -e \"a\\nb\\nc\\nd\"", "a\nb\nc\nd\n"),
            (r#"echo "hello\nworld""#, "hello\\nworld\n"),
            ("MYVAR=\"hello\"; echo $MYVAR", "hello\n"),
            ("MYVAR='hello'; echo $MYVAR", "hello\n"),
            (r#"MYVAR=""; echo "value:$MYVAR:""#, "value::\n"),
            ("MYVAR=''; echo \"value:$MYVAR:\"", "value::\n"),
            ("MYVAR=; echo \"value:$MYVAR:\"", "value::\n"),
            (r#"MYVAR="hello world"; echo "$MYVAR""#, "hello world\n"),
            (r#"export MYVAR=""; echo "value:$MYVAR:""#, "value::\n"),
        ] {
            let result = env_shell.exec(source);
            assert_eq!(result.stderr, "", "{source}");
            assert_eq!(result.stdout, expected_stdout, "{source}");
            assert_eq!(result.exit_code, 0, "{source}");
        }

        let mut op_shell = shell();
        for (source, expected_stdout, expected_exit) in [
            ("echo first && echo second", "first\nsecond\n", 0),
            ("false && echo second", "", 1),
            ("echo a && echo b && echo c && echo d", "a\nb\nc\nd\n", 0),
            ("echo a && false && echo b", "a\n", 1),
            ("false || echo fallback", "fallback\n", 0),
            ("echo success || echo fallback", "success\n", 0),
            ("false || false || echo fallback", "fallback\n", 0),
            ("echo first ; echo second", "first\nsecond\n", 0),
            ("false ; echo second", "second\n", 0),
            ("echo a;echo b;echo c", "a\nb\nc\n", 0),
            ("false && echo success || echo failure", "failure\n", 0),
            (
                "false || echo recovered && echo continued",
                "recovered\ncontinued\n",
                0,
            ),
            (
                "echo ok && echo next || echo skip ; echo done",
                "ok\nnext\ndone\n",
                0,
            ),
            ("echo hello | cat | cat | cat", "hello\n", 0),
            ("echo -e \"foo\\nbar\\nbaz\" | grep ba", "bar\nbaz\n", 0),
            ("echo test | grep missing && echo found", "", 1),
            ("echo test | grep test && echo found", "test\nfound\n", 0),
            (
                r#"echo test | grep missing || echo "not found""#,
                "not found\n",
                0,
            ),
        ] {
            let result = op_shell.exec(source);
            assert_eq!(result.stderr, "", "{source}");
            assert_eq!(result.stdout, expected_stdout, "{source}");
            assert_eq!(result.exit_code, expected_exit, "{source}");
        }

        for (source, path, expected_content) in [
            ("echo hello > /output.txt", "/output.txt", "hello\n"),
            ("echo new > /output.txt", "/output.txt", "new\n"),
            ("echo line2 >> /output.txt", "/output.txt", "new\nline2\n"),
            ("echo first >> /new.txt", "/new.txt", "first\n"),
            ("cat /input.txt > /copy.txt", "/copy.txt", "content\n"),
            (
                r#"echo -e "a\nb\nc" | grep b > /grep.txt"#,
                "/grep.txt",
                "b\n",
            ),
            ("echo test>/nospace.txt", "/nospace.txt", "test\n"),
            ("echo b>>/nospace.txt", "/nospace.txt", "test\nb\n"),
        ] {
            op_shell.files_mut().write("/input.txt", "content\n");
            let result = op_shell.exec(source);
            assert_eq!(result.stderr, "", "{source}");
            assert_eq!(result.exit_code, 0, "{source}");
            assert_eq!(
                op_shell.files().read_to_string(path),
                Some(expected_content),
                "{source}"
            );
        }

        for (source, expected_stdout, expected_exit) in [
            ("exit", "", 0),
            ("exit 42", "", 42),
            ("exit 1", "", 1),
            ("unknowncommand", "", 127),
            ("foobar", "", 127),
            ("", "", 0),
            ("   ", "", 0),
            ("   echo hello   ", "hello\n", 0),
            ("echo   hello   world", "hello world\n", 0),
            ("echo\thello\tworld", "hello world\n", 0),
        ] {
            let result = op_shell.exec(source);
            assert_eq!(result.stdout, expected_stdout, "{source}");
            assert_eq!(result.exit_code, expected_exit, "{source}");
        }

        let mut loop_shell = shell();
        loop_shell.files_mut().write("/file1.txt", "content1");
        loop_shell.files_mut().write("/file2.txt", "content2");
        for (source, expected_stdout, expected_exit) in [
            ("for i in a b c; do echo $i; done", "a\nb\nc\n", 0),
            ("for n in 1 2 3 4 5; do echo $n; done", "1\n2\n3\n4\n5\n", 0),
            ("for x in hello; do echo $x; done", "hello\n", 0),
            ("for x in; do echo $x; done", "", 0),
            (
                "for i in 1 2; do echo start $i; echo end $i; done",
                "start 1\nend 1\nstart 2\nend 2\n",
                0,
            ),
            (
                "for f in /file1.txt /file2.txt; do cat $f; done",
                "content1content2",
                0,
            ),
            ("for i in 1 2; do false; done", "", 1),
            (
                "for i in a b; do for j in 1 2; do echo $i$j; done; done",
                "a1\na2\nb1\nb2\n",
                0,
            ),
        ] {
            let result = loop_shell.exec(source);
            assert_eq!(result.stderr, "", "{source}");
            assert_eq!(result.stdout, expected_stdout, "{source}");
            assert_eq!(result.exit_code, expected_exit, "{source}");
        }
    }

    /// Mirrors portable `packages/just-bash/src/syntax/composition.test.ts`,
    /// `operators.test.ts`, and `loops.test.ts` syntax-feature-composition rows
    /// 1:1 through the Rust parser/interpreter. Each tuple is the exact upstream
    /// `Bash().exec(...)` source and its asserted stdout, exercising command
    /// substitution, here documents, arithmetic-in-here-doc, case words and
    /// branches, nested case, pipes inside blocks, for/function composition,
    /// nested command substitution, and the empty-here-doc/no-match edge rows.
    /// Rows requiring `mkdir`/`uniq`/`head`/`tail` command families or
    /// `[[ ]]` arithmetic comparison stay pending with their command owners.
    #[test]
    fn r10jb_syntax_composition_operator_and_loop_rows_match_upstream() {
        // composition.test.ts default-env rows.
        let plain: &[(&str, &str)] = &[
            // L6 command substitution in if condition
            (
                "\nif [[ $(echo hello) == \"hello\" ]]; then\necho \"matched\"\nfi\n",
                "matched\n",
            ),
            // L27 here document inside if block
            (
                "\nif [[ 1 -eq 1 ]]; then\ncat <<EOF\nhello from if\nEOF\nfi\n",
                "hello from if\n",
            ),
            // L53 pipes inside if block
            (
                "\nif [[ 1 -eq 1 ]]; then\necho -e \"line1\\nline2\\nline3\" | grep line2\nfi\n",
                "line2\n",
            ),
            // L65 pipe here document through multiple commands
            (
                "cat <<EOF | grep hello | wc -l\nhello world\ngoodbye world\nhello again\nEOF",
                "2\n",
            ),
            // L84 command substitution in here document
            (
                "cat <<EOF\nThe answer is $(echo 42)\nEOF",
                "The answer is 42\n",
            ),
            // L92 arithmetic expansion in here document
            ("cat <<EOF\n5 + 3 = $((5 + 3))\nEOF", "5 + 3 = 8\n"),
            // L114 command substitution as case word
            (
                "\ncase $(echo test) in\ntest) echo \"matched command output\";;\n*) echo \"no match\";;\nesac\n",
                "matched command output\n",
            ),
            // L125 arithmetic result as case word
            (
                "\ncase $((2 + 3)) in\n5) echo \"five\";;\n*) echo \"other\";;\nesac\n",
                "five\n",
            ),
            // L136 pipes inside case branch
            (
                "\ncase \"process\" in\nprocess)\necho -e \"a\\nb\\nc\" | wc -l\n;;\nesac\n",
                "3\n",
            ),
            // L148 here document inside case branch
            (
                "\ncase \"heredoc\" in\nheredoc)\ncat <<EOF\ninside case\nEOF\n;;\nesac\n",
                "inside case\n",
            ),
            // L162 nest case in case
            (
                "\ncase \"outer\" in\nouter)\ncase \"inner\" in\ninner) echo \"nested match\";;\nesac\n;;\nesac\n",
                "nested match\n",
            ),
            // L178 test command substitution result
            (
                "\nif [[ $(echo \"yes\") == \"yes\" ]]; then\necho \"command output matched\"\nfi\n",
                "command output matched\n",
            ),
            // L222 command substitution in for loop
            (
                "\nfor item in $(echo \"a b c\"); do\necho \"item: $item\"\ndone\n",
                "item: a\nitem: b\nitem: c\n",
            ),
            // L244 case statement inside loop
            (
                "\nfor fruit in apple banana cherry; do\ncase $fruit in\napple) echo \"red\";;\nbanana) echo \"yellow\";;\ncherry) echo \"red\";;\nesac\ndone\n",
                "red\nyellow\nred\n",
            ),
            // L258 here document inside loop
            (
                "\nfor i in 1 2; do\ncat <<EOF\niteration $i\nEOF\ndone\n",
                "iteration 1\niteration 2\n",
            ),
            // L270 pipe loop output
            ("\nfor i in 3 1 2; do\necho $i\ndone | sort\n", "1\n2\n3\n"),
            // L282 command substitution in function
            (
                "\ngreet() {\nlocal name=$(echo \"World\")\necho \"Hello, $name!\"\n}\ngreet\n",
                "Hello, World!\n",
            ),
            // L294 arithmetic in function
            ("\nadd() {\necho $(($1 + $2))\n}\nadd 5 3\n", "8\n"),
            // L305 case statement in function
            (
                "\nget_color() {\ncase $1 in\napple) echo \"red\";;\nbanana) echo \"yellow\";;\n*) echo \"unknown\";;\nesac\n}\nget_color apple\nget_color banana\nget_color grape\n",
                "red\nyellow\nunknown\n",
            ),
            // L339 here document in function
            (
                "\ngenerate_config() {\ncat <<EOF\nname=$1\nvalue=$2\nEOF\n}\ngenerate_config mykey myvalue\n",
                "name=mykey\nvalue=myvalue\n",
            ),
            // L353 call function with command substitution
            (
                "\ndouble() {\necho $(($1 * 2))\n}\nresult=$(double 5)\necho \"Result: $result\"\n",
                "Result: 10\n",
            ),
            // L367 combine if, case, and command substitution
            (
                "\nexport TYPE=$(echo \"fruit\")\nif [[ $TYPE == \"fruit\" ]]; then\ncase $(echo apple) in\napple) echo \"it's an apple\";;\n*) echo \"unknown fruit\";;\nesac\nfi\n",
                "it's an apple\n",
            ),
            // L381 here doc with command substitution and pipes
            (
                "\nexport PREFIX=\">>>\"\ncat <<EOF | grep world\n$PREFIX hello\n$PREFIX world\n$PREFIX $(echo \"dynamic\")\nEOF",
                ">>> world\n",
            ),
            // L455 nested command substitution
            (
                "\necho \"Result: $(echo \"inner: $(echo deep)\")\"\n",
                "Result: inner: deep\n",
            ),
            // L479 failed command in command substitution
            (
                "\nresult=$(cat /nonexistent/file 2>/dev/null)\nif [[ -z \"$result\" ]]; then\necho \"no result\"\nfi\n",
                "no result\n",
            ),
            // L490 empty here document in pipe
            ("cat <<EOF | wc -l\nEOF", "0\n"),
            // L497 case with no matching pattern
            (
                "\ncase \"nomatch\" in\na) echo \"a\";;\nb) echo \"b\";;\nesac\necho \"done\"\n",
                "done\n",
            ),
            // operators.test.ts L241 count lines with wc in pipe
            ("echo -e \"a\\nb\\nc\" | wc -l", "3\n"),
        ];
        for (source, expected) in plain {
            let mut env_shell = shell();
            let result = env_shell.exec(source);
            assert_eq!(result.stdout, *expected, "{source}");
            assert_eq!(result.exit_code, 0, "{source}");
        }

        // composition.test.ts L75 uses a PATTERN env entry.
        let mut pattern_shell = shell().with_env([("PATTERN", "world")]);
        let result =
            pattern_shell.exec("cat <<EOF | grep $PATTERN\nhello world\ngoodbye moon\nEOF");
        assert_eq!(result.stdout, "hello world\n");
        assert_eq!(result.exit_code, 0);

        // loops.test.ts L124 until loop executes when condition is initially
        // false: a separate-exec pattern that seeds the file, then runs one
        // iteration before the until condition becomes true.
        let mut until_shell = shell();
        until_shell.exec("echo no > /check.txt");
        let result = until_shell
            .exec("until grep -q yes /check.txt; do echo step; echo yes > /check.txt; done");
        assert_eq!(result.stdout, "step\n");
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn jbc33_syntax_control_flow_functions_and_local_rows_match_upstream() {
        let mut interp = shell();
        interp.files_mut().write("/test.txt", "hello\nworld\n");
        for (source, expected_stdout, expected_exit) in [
            ("if true; then echo yes; fi", "yes\n", 0),
            ("if false; then echo yes; fi", "", 0),
            ("if false; then echo yes; else echo no; fi", "no\n", 0),
            (
                "if grep hello /test.txt > /dev/null; then echo found; fi",
                "found\n",
                0,
            ),
            (
                "if false; then echo one; elif true; then echo two; else echo three; fi",
                "two\n",
                0,
            ),
            (
                "if false; then echo 1; elif false; then echo 2; elif true; then echo 3; else echo 4; fi",
                "3\n",
                0,
            ),
            (
                "if cat /test.txt | grep world > /dev/null; then echo found; fi",
                "found\n",
                0,
            ),
            (
                "if true; then echo one; echo two; echo three; fi",
                "one\ntwo\nthree\n",
                0,
            ),
            (
                "if true; then if true; then echo nested; fi; fi",
                "nested\n",
                0,
            ),
            (
                "if true; then if true; then if true; then echo deep; fi; fi; fi",
                "deep\n",
                0,
            ),
            (
                "check() { if true; then echo inside; fi; }; check",
                "inside\n",
                0,
            ),
            (
                "if false; then echo one; else if true; then echo two; fi; fi",
                "two\n",
                0,
            ),
            (
                "echo before; if true; then echo during; fi; echo after",
                "before\nduring\nafter\n",
                0,
            ),
            ("function greet { echo hello; }; greet", "hello\n", 0),
            ("greet() { echo hello; }; greet", "hello\n", 0),
            (
                "greet() { echo Hello $1; }; greet World",
                "Hello World\n",
                0,
            ),
            ("count() { echo $#; }; count a b c", "3\n", 0),
            (
                "show() { echo $@; }; show one two three",
                "one two three\n",
                0,
            ),
            (
                "multi() { echo first; echo second; echo third; }; multi",
                "first\nsecond\nthird\n",
                0,
            ),
            (
                "inner() { echo inside; }; outer() { echo before; inner; echo after; }; outer",
                "before\ninside\nafter\n",
                0,
            ),
            ("fail() { echo hi; false; }; fail", "hi\n", 1),
            ("echo() { true; }; echo hello", "", 0),
            (
                "test_func() { local x=hello; echo $x; }; test_func",
                "hello\n",
                0,
            ),
            (
                "test_func() { local a=1 b=2 c=3; echo $a $b $c; }; test_func",
                "1 2 3\n",
                0,
            ),
        ] {
            let result = interp.exec(source);
            assert_eq!(result.stderr, "", "{source}");
            assert_eq!(result.stdout, expected_stdout, "{source}");
            assert_eq!(result.exit_code, expected_exit, "{source}");
        }

        let parse_error = interp.exec("if true; then echo hello");
        assert_eq!(parse_error.exit_code, 2);
        assert!(parse_error.stderr.contains("syntax error"));

        let outside_local = interp.exec("local x=value");
        assert_eq!(outside_local.exit_code, 1);
        assert!(
            outside_local
                .stderr
                .contains("can only be used in a function")
        );
    }

    #[test]
    fn jbc33_interpreter_prototype_keywords_remain_plain_shell_data() {
        let mut interp = shell();
        for keyword in [
            "constructor",
            "__proto__",
            "prototype",
            "hasOwnProperty",
            "toString",
            "valueOf",
        ] {
            let result = interp.exec(&format!("echo {keyword}"));
            assert_eq!(result.exit_code, 0, "{keyword}");
            assert_eq!(result.stdout, format!("{keyword}\n"), "{keyword}");
        }

        for (source, expected_stdout) in [
            ("constructor=test; echo $constructor", "test\n"),
            ("__proto__=test; echo $__proto__", "test\n"),
            ("prototype=test; echo $prototype", "test\n"),
            ("hasOwnProperty=test; echo $hasOwnProperty", "test\n"),
            ("echo $constructor", "\n"),
            ("echo $__proto__", "\n"),
            ("echo $prototype", "\n"),
            (
                "x=\"test constructor test\"; echo $x",
                "test constructor test\n",
            ),
            (
                "x=\"test __proto__ test\"; echo $x",
                "test __proto__ test\n",
            ),
            ("constructor() { echo 'func'; }; constructor", "func\n"),
            ("__proto__() { echo 'func'; }; __proto__", "func\n"),
            ("echo $(echo constructor)", "constructor\n"),
            ("echo $(echo __proto__)", "__proto__\n"),
            ("constructor=5; echo $((constructor + 3))", "8\n"),
            ("__proto__=5; echo $((__proto__ + 3))", "8\n"),
            (
                "for constructor in a b c; do echo $constructor; done",
                "a\nb\nc\n",
            ),
            (
                "for x in constructor __proto__ prototype; do echo $x; done",
                "constructor\n__proto__\nprototype\n",
            ),
            (
                "x=constructor; case $x in constructor) echo matched;; *) echo nomatch;; esac",
                "matched\n",
            ),
            ("echo .constructor", ".constructor\n"),
            ("echo '[constructor]'", "[constructor]\n"),
            ("echo '{constructor}'", "{constructor}\n"),
            ("echo __proto__.test", "__proto__.test\n"),
            (
                "testfunc() { local __proto__=local_value; echo $__proto__; }; testfunc",
                "local_value\n",
            ),
            (
                "cat <<EOF\n__proto__\nconstructor\nprototype\nEOF",
                "__proto__\nconstructor\nprototype\n",
            ),
            ("cat <<__proto__\ntest content\n__proto__", "test content\n"),
            (
                "echo {__proto__,constructor,prototype}",
                "__proto__ constructor prototype\n",
            ),
            (
                "echo test_{__proto__,constructor}",
                "test___proto__ test_constructor\n",
            ),
            ("export __proto__=passed; echo $__proto__", "passed\n"),
            (
                "__proto__=set; unset __proto__; echo \"value: '$__proto__'\"",
                "value: ''\n",
            ),
            (
                "result=$(echo __proto__); echo \"got: $result\"",
                "got: __proto__\n",
            ),
            ("( __proto__=subshell; echo $__proto__ )", "subshell\n"),
        ] {
            let mut case_shell = shell();
            let result = case_shell.exec(source);
            assert_eq!(result.stderr, "", "{source}");
            assert_eq!(result.exit_code, 0, "{source}");
            assert_eq!(result.stdout, expected_stdout, "{source}");
        }
    }

    #[test]
    fn jbc41_interpreter_xtrace_set_x_ps4_and_execution_rows() {
        let mut basic = shell();
        let result = basic.exec(
            "set -x
echo hello
echo one two three
set +x
echo not traced",
        );
        assert_eq!(result.stdout, "hello\none two three\nnot traced\n");
        assert!(result.stderr.contains("+ echo hello"));
        assert!(result.stderr.contains("+ echo one two three"));
        assert!(result.stderr.contains("+ set +x"));
        assert!(!result.stderr.contains("+ echo not traced"));
        assert_eq!(result.exit_code, 0);

        let mut ps4 = shell();
        let result = ps4.exec(
            "PS4='>>> '
set -x
echo test",
        );
        assert!(result.stderr.contains(">>> echo test"));

        let mut ps4_vars = shell();
        let result = ps4_vars.exec(
            "MYVAR=DEBUG
PS4='[$MYVAR] '
set -x
echo test",
        );
        assert!(result.stderr.contains("[DEBUG] echo test"));

        let mut ps4_lineno = shell();
        let result = ps4_lineno.exec(
            "PS4='+$LINENO: '
set -x
echo line1",
        );
        assert!(result.stderr.contains(": echo line1"));

        let mut empty_ps4 = shell();
        let result = empty_ps4.exec(
            "PS4=''
set -x
echo test",
        );
        assert!(result.stderr.contains("echo test"));

        let mut quoting = shell();
        let result = quoting.exec(
            "set -x
echo \"hello world\"
echo \"\"
printf 'a\\nb'
x=5
echo $x
FOO=bar echo hello",
        );
        assert_eq!(result.stdout, "hello world\n\na\nb5\nhello\n");
        assert!(result.stderr.contains("hello world"));
        assert!(result.stderr.contains("+ echo ''"));
        assert!(result.stderr.contains("+ printf"));
        assert!(result.stderr.contains("+ x=5"));
        assert!(result.stderr.contains("+ FOO=bar echo hello"));

        let mut control = shell();
        let result = control.exec(
            "set -x
for i in 1 2; do echo $i; done
x=0
while [ $x -lt 2 ]; do echo $x; x=$((x + 1)); done
if true; then echo yes; else echo no; fi",
        );
        assert_eq!(result.stdout, "1\n2\n0\n1\nyes\n");
        assert!(result.stderr.contains("+ echo 1"));
        assert!(result.stderr.contains("+ echo 2"));
        assert!(result.stderr.contains("+ '[' 0 -lt 2 ']'"));
        assert!(result.stderr.contains("+ echo 0"));
        assert!(result.stderr.contains("+ true"));
        assert!(result.stderr.contains("+ echo yes"));
        assert!(!result.stderr.contains("+ echo no"));

        let mut traced_subshell = shell();
        let result = traced_subshell.exec(
            "set -x
(echo subshell)",
        );
        assert_eq!(result.stdout, "subshell\n");
        assert!(result.stderr.contains("+ echo subshell"));

        let mut subshell = shell();
        let result = subshell.exec(
            "set -x
(set +x; echo subshell)
echo after",
        );
        assert_eq!(result.stdout, "subshell\nafter\n");
        assert!(result.stderr.contains("+ set +x"));
        assert!(result.stderr.contains("+ echo after"));
        assert!(!result.stderr.contains("+ echo subshell"));

        let mut pipeline = shell();
        let result = pipeline.exec(
            "set -x
echo hello | cat",
        );
        assert_eq!(result.stdout, "hello\n");
        assert!(result.stderr.contains("+ echo hello"));
        assert!(result.stderr.contains("+ cat"));

        let mut functions = shell();
        let result = functions.exec(
            "greet() { echo \"Hello $1\"; }
set -x
greet World",
        );
        assert_eq!(result.stdout, "Hello World\n");
        assert!(result.stderr.contains("+ greet World"));
        assert!(result.stderr.contains("+ echo 'Hello World'"));
    }

    #[test]
    fn jbc33_transform_exec_metadata_rows_are_portable() {
        #[derive(Default)]
        struct CustomPlugin;

        impl TransformPlugin for CustomPlugin {
            fn name(&self) -> &str {
                "custom"
            }

            fn transform(&mut self, context: TransformContext<'_>) -> ShellResult<TransformResult> {
                Ok(TransformResult {
                    ast: context.ast.clone(),
                    metadata: TransformMetadata {
                        custom: BTreeMap::from([("custom".to_string(), true)]),
                        ..TransformMetadata::default()
                    },
                })
            }
        }

        struct RewritePlugin;

        impl TransformPlugin for RewritePlugin {
            fn name(&self) -> &str {
                "rewrite"
            }

            fn transform(
                &mut self,
                _context: TransformContext<'_>,
            ) -> ShellResult<TransformResult> {
                Ok(TransformResult {
                    ast: parse("echo transformed").expect("rewrite parses"),
                    metadata: TransformMetadata {
                        custom: BTreeMap::from([("rewritten".to_string(), true)]),
                        ..TransformMetadata::default()
                    },
                })
            }
        }

        let mut collector = BashTransformPipeline::new().use_plugin(CommandCollectorPlugin);
        let collected = collector.transform("echo hello | cat").unwrap();
        let mut transformed_shell = shell();
        assert_eq!(transformed_shell.exec(&collected.script).stdout, "hello\n");
        assert_eq!(collected.metadata.commands, vec!["cat", "echo"]);

        let mut rewrite = BashTransformPipeline::new().use_plugin(RewritePlugin);
        let rewritten = rewrite.transform("echo original").unwrap();
        assert_eq!(shell().exec(&rewritten.script).stdout, "transformed\n");
        assert_eq!(rewritten.metadata.custom.get("rewritten"), Some(&true));

        let mut no_plugins = BashTransformPipeline::new();
        let plain = no_plugins.transform("echo hello").unwrap();
        assert!(plain.metadata.is_empty());
        assert_eq!(shell().exec(&plain.script).stdout, "hello\n");

        let mut merged = BashTransformPipeline::new()
            .use_plugin(CommandCollectorPlugin)
            .use_plugin(CustomPlugin);
        let result = merged.transform("echo hello").unwrap();
        assert_eq!(result.metadata.commands, vec!["echo"]);
        assert_eq!(result.metadata.custom.get("custom"), Some(&true));
    }

    #[test]
    fn jbc33_user_regex_portable_match_search_split_and_replace_rows() {
        let foo = Regex::new("foo").unwrap();
        assert_eq!(foo.as_str(), "foo");
        assert!(foo.is_match("foobar"));
        assert!(!foo.is_match("bar"));
        assert!(Regex::new("(?i)foo").unwrap().is_match("FOO"));
        let invalid_pattern = String::from("[");
        assert!(
            Regex::new(&invalid_pattern)
                .expect_err("invalid pattern should fail")
                .to_string()
                .contains("unclosed")
        );

        let captures = Regex::new("f(o+)").unwrap().captures("foobar").unwrap();
        assert_eq!(captures.get(0).unwrap().as_str(), "foo");
        assert_eq!(captures.get(1).unwrap().as_str(), "oo");
        assert_eq!(captures.get(0).unwrap().start(), 0);
        assert_eq!(
            Regex::new("o+")
                .unwrap()
                .find_iter("foobooo")
                .map(|match_| match_.as_str())
                .collect::<Vec<_>>(),
            vec!["oo", "ooo"]
        );
        assert_eq!(
            Regex::new(",\\s*")
                .unwrap()
                .split("a, b,  c")
                .collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
        assert_eq!(
            Regex::new(",")
                .unwrap()
                .split("a,b,c,d")
                .take(2)
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        assert_eq!(
            Regex::new("x").unwrap().split("abc").collect::<Vec<_>>(),
            vec!["abc"]
        );
        assert_eq!(
            Regex::new(",").unwrap().split("a,,b").collect::<Vec<_>>(),
            vec!["a", "", "b"]
        );

        let first_o = Regex::new("o").unwrap();
        assert_eq!(first_o.replace("foobar", "0"), "f0obar");
        assert_eq!(
            Regex::new("o").unwrap().replace_all("foobar", "0"),
            "f00bar"
        );
        assert_eq!(
            Regex::new("(f)(o+)").unwrap().replace("foobar", "$2$1"),
            "oofbar"
        );
        assert_eq!(
            Regex::new("\\d+").unwrap().replace_all("a1b22c333", "[$0]"),
            "a[1]b[22]c[333]"
        );
        assert_eq!(
            Regex::new("bar").unwrap().find("foobar").unwrap().start(),
            3
        );
        assert!(Regex::new("xyz").unwrap().find("foobar").is_none());
        assert!(Regex::new("\\[\\]\\(\\)").unwrap().is_match("[]()"));
        assert_eq!(
            Regex::new("[a-z]+")
                .unwrap()
                .find("ABC123def")
                .unwrap()
                .as_str(),
            "def"
        );
        assert!(Regex::new("^foo$").unwrap().is_match("foo"));
        assert!(!Regex::new("^foo$").unwrap().is_match("foobar"));
        assert!(Regex::new("").unwrap().is_match("anything"));
        assert_eq!(
            Regex::new("\\s+")
                .unwrap()
                .split("a b  c")
                .collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
        assert!(Regex::new("café").unwrap().is_match("I love café"));
        assert!(Regex::new("\\x{1F600}").unwrap().is_match("Hello 😀"));
        assert!(Regex::new("(?s)a.b").unwrap().is_match("a\nb"));
        assert!(!Regex::new("a.b").unwrap().is_match("a\nb"));
        let named = Regex::new("(?P<year>\\d{4})-(?P<month>\\d{2})").unwrap();
        let named_captures = named.captures("2024-01").unwrap();
        assert_eq!(&named_captures["year"], "2024");
        assert_eq!(&named_captures["month"], "01");
        let nested = Regex::new("((a)(b))").unwrap().captures("ab").unwrap();
        assert_eq!(nested.get(0).unwrap().as_str(), "ab");
        assert_eq!(nested.get(1).unwrap().as_str(), "ab");
        assert_eq!(nested.get(2).unwrap().as_str(), "a");
        assert_eq!(nested.get(3).unwrap().as_str(), "b");
        let non_capturing = Regex::new("(?:a)(b)").unwrap().captures("ab").unwrap();
        assert_eq!(non_capturing.get(0).unwrap().as_str(), "ab");
        assert_eq!(non_capturing.get(1).unwrap().as_str(), "b");
        assert!(non_capturing.get(2).is_none());
    }

    // JBC-33: additional portable user-regex rows mirroring the interleaved
    // `test`/`it` cases in packages/just-bash/src/regex/user-regex.test.ts that
    // exercise membership testing, zero-length global matching, multiline
    // semantics, cached-matcher reuse, anchors, the empty pattern, Unicode
    // literals/escapes, and the dotAll flag. The JavaScript-only lastIndex
    // state and native RegExp wrapper rows stay documented separately; here we
    // assert only the observable, portable regex semantics those rows verify.
    #[test]
    fn jbc33_user_regex_portable_membership_zero_length_multiline_and_unicode_rows() {
        // test(): membership predicate (lines 33, 38, 43, 49, 249, 255, 393).
        assert!(Regex::new("foo").unwrap().is_match("foobar"));
        assert!(!Regex::new("foo").unwrap().is_match("bar"));
        assert!(Regex::new("(?i)foo").unwrap().is_match("FOO"));
        // Global `a` against "aaa" still reports a match regardless of any
        // JS lastIndex bookkeeping; the portable result is a plain match.
        assert!(Regex::new("a").unwrap().is_match("aaa"));
        // ConstantRegex `/foo/` and `/a/g` rows observe the same membership.
        assert!(Regex::new("foo").unwrap().is_match("foobar"));
        assert!(Regex::new("a").unwrap().is_match("aaa"));
        // RegexLike compatibility row: `\d+` against "a1b22c333" matches.
        assert!(Regex::new("\\d+").unwrap().is_match("a1b22c333"));

        // matchAll(): zero-length word-boundary matches (line 200). Boundaries
        // surround each word: before/after 'a' and before/after 'b' = 4.
        assert_eq!(Regex::new("\\b").unwrap().find_iter("a b").count(), 4);

        // multiline (line 223): `^foo` with the multiline flag anchors to the
        // start of each line, so it matches the second line here; without the
        // flag the same pattern only anchors at the very start of the input.
        assert!(Regex::new("(?m)^foo").unwrap().is_match("bar\nfoo"));
        assert!(!Regex::new("^foo").unwrap().is_match("bar\nfoo"));

        // acquireMatcher reuse (lines 404, 412): repeated calls on one compiled
        // pattern keep returning correct independent results.
        let o_plus = Regex::new("o+").unwrap();
        assert_eq!(
            o_plus
                .find_iter("foooo bar ooo")
                .map(|m| m.as_str())
                .collect::<Vec<_>>(),
            vec!["oooo", "ooo"]
        );
        assert!(o_plus.find("bar baz").is_none());
        let foo_global = Regex::new("foo").unwrap();
        assert_eq!(foo_global.replace_all("foo bar foo", "baz"), "baz bar baz");
        assert_eq!(foo_global.replace_all("foo only once", "X"), "X only once");

        // edge cases: escaped special chars (453), anchors (463/464),
        // empty pattern (471), Unicode literal (483) and escape with the u
        // flag (488), and the dotAll flag (495/500).
        assert!(Regex::new("\\[\\]\\(\\)").unwrap().is_match("[]()"));
        assert!(Regex::new("^foo$").unwrap().is_match("foo"));
        assert!(!Regex::new("^foo$").unwrap().is_match("foobar"));
        assert!(Regex::new("").unwrap().is_match("anything"));
        assert!(Regex::new("café").unwrap().is_match("I love café"));
        assert!(Regex::new("\\x{1F600}").unwrap().is_match("Hello 😀"));
        assert!(Regex::new("(?s)a.b").unwrap().is_match("a\nb"));
        assert!(!Regex::new("a.b").unwrap().is_match("a\nb"));
    }

    // JBC-13: portable break/continue conformance mirroring
    // packages/just-bash/src/syntax/break-continue.test.ts (all 12 upstream cases).
    #[test]
    fn jbc13_syntax_break_continue_matches_upstream_behavior() {
        // break: exit for loop early
        let result = shell().exec(
            "for i in 1 2 3 4 5; do\n  if [ $i -eq 3 ]; then break; fi\n  echo $i\ndone\necho done",
        );
        assert_eq!(result.stdout, "1\n2\ndone\n");
        assert_eq!(result.exit_code, 0);

        // break: exit while loop early
        let result = shell().exec(
            "x=0\nwhile [ $x -lt 10 ]; do\n  x=$((x + 1))\n  if [ $x -eq 3 ]; then break; fi\n  echo $x\ndone\necho done",
        );
        assert_eq!(result.stdout, "1\n2\ndone\n");
        assert_eq!(result.exit_code, 0);

        // break: exit until loop early
        let result = shell().exec(
            "x=0\nuntil [ $x -ge 10 ]; do\n  x=$((x + 1))\n  if [ $x -eq 3 ]; then break; fi\n  echo $x\ndone\necho done",
        );
        assert_eq!(result.stdout, "1\n2\ndone\n");
        assert_eq!(result.exit_code, 0);

        // break: break multiple levels with `break n`
        let result = shell().exec(
            "for i in 1 2; do\n  for j in a b c; do\n    if [ $j = b ]; then break 2; fi\n    echo \"$i$j\"\n  done\ndone\necho done",
        );
        assert_eq!(result.stdout, "1a\ndone\n");
        assert_eq!(result.exit_code, 0);

        // break: silently do nothing when not in loop
        let result = shell().exec("break");
        assert_eq!(result.stderr, "");
        assert_eq!(result.exit_code, 0);

        // break: error on invalid argument (bash returns 128)
        let result = shell().exec("for i in 1 2 3; do\n  break abc\ndone");
        assert!(
            result.stderr.contains("numeric argument required"),
            "stderr was: {:?}",
            result.stderr
        );
        assert_eq!(result.exit_code, 128);

        // continue: skip to next iteration in for loop
        let result = shell().exec(
            "for i in 1 2 3 4 5; do\n  if [ $i -eq 3 ]; then continue; fi\n  echo $i\ndone\necho done",
        );
        assert_eq!(result.stdout, "1\n2\n4\n5\ndone\n");
        assert_eq!(result.exit_code, 0);

        // continue: skip to next iteration in while loop
        let result = shell().exec(
            "x=0\nwhile [ $x -lt 5 ]; do\n  x=$((x + 1))\n  if [ $x -eq 3 ]; then continue; fi\n  echo $x\ndone\necho done",
        );
        assert_eq!(result.stdout, "1\n2\n4\n5\ndone\n");
        assert_eq!(result.exit_code, 0);

        // continue: continue multiple levels with `continue n`
        let result = shell().exec(
            "for i in 1 2; do\n  for j in a b c; do\n    if [ $j = b ]; then continue 2; fi\n    echo \"$i$j\"\n  done\n  echo \"end-$i\"\ndone\necho done",
        );
        assert_eq!(result.stdout, "1a\n2a\ndone\n");
        assert_eq!(result.exit_code, 0);

        // continue: silently do nothing when not in loop
        let result = shell().exec("continue");
        assert_eq!(result.stderr, "");
        assert_eq!(result.exit_code, 0);

        // nested control flow: case statements inside loops
        let result = shell()
            .exec("for x in a b c; do\n  case $x in\n    b) continue ;;\n  esac\n  echo $x\ndone");
        assert_eq!(result.stdout, "a\nc\n");
        assert_eq!(result.exit_code, 0);

        // nested control flow: break inside subshell only breaks that subshell
        let result = shell().exec(
            "for i in 1 2 3; do\n  (\n    if [ $i -eq 2 ]; then break; fi\n    echo $i\n  )\ndone\necho done",
        );
        assert_eq!(result.stdout, "1\n3\ndone\n");
    }

    /// Mirrors every `it(...)` in
    /// `packages/just-bash/src/interpreter/builtins/break.test.ts` 1:1, exercising
    /// the Rust `break` builtin over the virtual shell.
    #[test]
    fn r5_interpreter_builtin_break_matches_upstream() {
        // L6 exit for loop early
        let result = shell().exec(
            "for i in 1 2 3 4 5; do\n  if [ $i -eq 3 ]; then break; fi\n  echo $i\ndone\necho done",
        );
        assert_eq!(result.stdout, "1\n2\ndone\n");
        assert_eq!(result.exit_code, 0);

        // L19 exit while loop early
        let result = shell().exec(
            "x=0\nwhile [ $x -lt 10 ]; do\n  x=$((x + 1))\n  if [ $x -eq 3 ]; then break; fi\n  echo $x\ndone\necho done",
        );
        assert_eq!(result.stdout, "1\n2\ndone\n");
        assert_eq!(result.exit_code, 0);

        // L34 exit until loop early
        let result = shell().exec(
            "x=0\nuntil [ $x -ge 10 ]; do\n  x=$((x + 1))\n  if [ $x -eq 3 ]; then break; fi\n  echo $x\ndone\necho done",
        );
        assert_eq!(result.stdout, "1\n2\ndone\n");
        assert_eq!(result.exit_code, 0);

        // L51 break multiple levels with `break n`
        let result = shell().exec(
            "for i in 1 2; do\n  for j in a b c; do\n    if [ $j = b ]; then break 2; fi\n    echo \"$i$j\"\n  done\ndone\necho done",
        );
        assert_eq!(result.stdout, "1a\ndone\n");
        assert_eq!(result.exit_code, 0);

        // L66 break single level with `break 1`
        let result = shell().exec(
            "for i in 1 2 3; do\n  if [ $i -eq 2 ]; then break 1; fi\n  echo $i\ndone\necho done",
        );
        assert_eq!(result.stdout, "1\ndone\n");

        // L78 break with level exceeding loop depth just breaks out
        let result = shell().exec("for i in 1 2; do\n  break 10\n  echo $i\ndone\necho done");
        assert_eq!(result.stdout, "done\n");

        // L93 break outside a loop is a silent no-op
        let result = shell().exec("break");
        assert_eq!(result.stderr, "");
        assert_eq!(result.exit_code, 0);

        // L101 break with a non-numeric argument: numeric argument required, code 128
        let result = shell().exec("for i in 1 2 3; do\n  break abc\ndone");
        assert!(
            result.stderr.contains("numeric argument required"),
            "stderr was: {:?}",
            result.stderr
        );
        assert_eq!(result.exit_code, 128);

        // L112 break 0 is invalid: numeric argument required, code 128
        let result = shell().exec("for i in 1 2 3; do\n  break 0\ndone");
        assert!(result.stderr.contains("numeric argument required"));
        assert_eq!(result.exit_code, 128);

        // L123 break -1 is invalid: numeric argument required, code 128
        let result = shell().exec("for i in 1 2 3; do\n  break -1\ndone");
        assert!(result.stderr.contains("numeric argument required"));
        assert_eq!(result.exit_code, 128);

        // L134 break with too many arguments errors with code 1
        let result = shell().exec("for x in a b c; do\n  echo $x\n  break 1 2 3\ndone\necho --");
        assert_eq!(result.stdout, "a\n");
        assert!(result.stderr.contains("too many arguments"));
        assert_eq!(result.exit_code, 1);

        // L151 break works with case statements inside loops
        let result = shell().exec(
            "for x in a b c; do\n  case $x in\n    b) break ;;\n  esac\n  echo $x\ndone\necho done",
        );
        assert_eq!(result.stdout, "a\ndone\n");

        // L165 break works with if statements inside loops
        let result = shell().exec(
            "for i in 1 2 3 4 5; do\n  if [ $i -gt 2 ]; then\n    break\n  fi\n  echo $i\ndone",
        );
        assert_eq!(result.stdout, "1\n2\n");

        // L178 break in a function inside a loop breaks the outer loop
        let result = shell().exec(
            "check() {\n  if [ $1 -eq 3 ]; then\n    break\n  fi\n}\nfor i in 1 2 3 4 5; do\n  check $i\n  echo $i\ndone\necho done",
        );
        assert_eq!(result.stdout, "1\n2\ndone\n");
    }

    /// Mirrors every `it(...)` in
    /// `packages/just-bash/src/interpreter/builtins/continue.test.ts` 1:1,
    /// exercising the Rust `continue` builtin over the virtual shell.
    #[test]
    fn r5_interpreter_builtin_continue_matches_upstream() {
        // L6 skip to next iteration in for loop
        let result = shell().exec(
            "for i in 1 2 3 4 5; do\n  if [ $i -eq 3 ]; then continue; fi\n  echo $i\ndone\necho done",
        );
        assert_eq!(result.stdout, "1\n2\n4\n5\ndone\n");
        assert_eq!(result.exit_code, 0);

        // L19 skip to next iteration in while loop
        let result = shell().exec(
            "x=0\nwhile [ $x -lt 5 ]; do\n  x=$((x + 1))\n  if [ $x -eq 3 ]; then continue; fi\n  echo $x\ndone\necho done",
        );
        assert_eq!(result.stdout, "1\n2\n4\n5\ndone\n");
        assert_eq!(result.exit_code, 0);

        // L34 skip to next iteration in until loop
        let result = shell().exec(
            "x=0\nuntil [ $x -ge 5 ]; do\n  x=$((x + 1))\n  if [ $x -eq 3 ]; then continue; fi\n  echo $x\ndone\necho done",
        );
        assert_eq!(result.stdout, "1\n2\n4\n5\ndone\n");
        assert_eq!(result.exit_code, 0);

        // L51 continue multiple levels with `continue n`
        let result = shell().exec(
            "for i in 1 2; do\n  for j in a b c; do\n    if [ $j = b ]; then continue 2; fi\n    echo \"$i$j\"\n  done\n  echo \"end-$i\"\ndone\necho done",
        );
        assert_eq!(result.stdout, "1a\n2a\ndone\n");
        assert_eq!(result.exit_code, 0);

        // L67 continue single level with `continue 1`
        let result = shell()
            .exec("for i in 1 2 3; do\n  if [ $i -eq 2 ]; then continue 1; fi\n  echo $i\ndone");
        assert_eq!(result.stdout, "1\n3\n");

        // L78 continue with level exceeding loop depth continues the current loop
        let result = shell().exec(
            "for i in 1 2 3; do\n  if [ $i -eq 2 ]; then continue 10; fi\n  echo $i\ndone\necho done",
        );
        assert_eq!(result.stdout, "1\n3\ndone\n");

        // L93 continue outside a loop is a silent no-op
        let result = shell().exec("continue");
        assert_eq!(result.stderr, "");
        assert_eq!(result.exit_code, 0);

        // L101 continue with a non-numeric argument: numeric argument required, code 1
        let result = shell().exec("for i in 1 2 3; do\n  continue abc\ndone");
        assert!(
            result.stderr.contains("numeric argument required"),
            "stderr was: {:?}",
            result.stderr
        );
        assert_eq!(result.exit_code, 1);

        // L112 continue 0 is invalid: numeric argument required, code 1
        let result = shell().exec("for i in 1 2 3; do\n  continue 0\ndone");
        assert!(result.stderr.contains("numeric argument required"));
        assert_eq!(result.exit_code, 1);

        // L123 continue -1 is invalid: numeric argument required, code 1
        let result = shell().exec("for i in 1 2 3; do\n  continue -1\ndone");
        assert!(result.stderr.contains("numeric argument required"));
        assert_eq!(result.exit_code, 1);

        // L134 continue with too many arguments errors with code 1
        let result = shell().exec("for x in a b c; do\n  echo $x\n  continue 1 2 3\ndone\necho --");
        assert_eq!(result.stdout, "a\n");
        assert!(result.stderr.contains("too many arguments"));
        assert_eq!(result.exit_code, 1);

        // L151 continue works with case statements inside loops
        let result = shell()
            .exec("for x in a b c; do\n  case $x in\n    b) continue ;;\n  esac\n  echo $x\ndone");
        assert_eq!(result.stdout, "a\nc\n");
        assert_eq!(result.exit_code, 0);

        // L165 continue works with if statements inside loops
        let result = shell().exec(
            "for i in 1 2 3 4 5; do\n  if [ $i -eq 2 ] || [ $i -eq 4 ]; then\n    continue\n  fi\n  echo $i\ndone",
        );
        assert_eq!(result.stdout, "1\n3\n5\n");

        // L178 continue in a function inside a loop continues the outer loop
        let result = shell().exec(
            "skip_even() {\n  if [ $(($1 % 2)) -eq 0 ]; then\n    continue\n  fi\n}\nfor i in 1 2 3 4 5; do\n  skip_even $i\n  echo $i\ndone",
        );
        assert_eq!(result.stdout, "1\n3\n5\n");

        // L197/L208 (continue in a C-style `for (( ))` loop) are intentionally
        // excluded: the Rust interpreter does not yet implement C-style for
        // loops, so those two upstream rows stay pending.
    }

    // JBC-13: portable `set -o pipefail` conformance mirroring
    // packages/just-bash/src/syntax/set-pipefail.test.ts (all 9 upstream cases).
    #[test]
    fn jbc13_syntax_set_pipefail_matches_upstream_behavior() {
        // success when all commands succeed
        let result = shell().exec("set -o pipefail\necho hello | cat | cat\necho \"exit: $?\"");
        assert_eq!(result.stdout, "hello\nexit: 0\n");
        assert_eq!(result.exit_code, 0);

        // failure when first command fails
        let result = shell().exec("set -o pipefail\nfalse | true\necho \"exit: $?\"");
        assert_eq!(result.stdout, "exit: 1\n");
        assert_eq!(result.exit_code, 0);

        // failure when middle command fails
        let result = shell().exec("set -o pipefail\necho hello | false | cat\necho \"exit: $?\"");
        assert_eq!(result.stdout, "exit: 1\n");
        assert_eq!(result.exit_code, 0);

        // rightmost failing exit code
        let result = shell().exec("set -o pipefail\nexit 2 | exit 3 | true\necho \"exit: $?\"");
        assert_eq!(result.stdout, "exit: 3\n");
        assert_eq!(result.exit_code, 0);

        // without pipefail: last command exit code
        let result = shell().exec("false | true\necho \"exit: $?\"");
        assert_eq!(result.stdout, "exit: 0\n");
        assert_eq!(result.exit_code, 0);

        // disable pipefail with +o pipefail
        let result =
            shell().exec("set -o pipefail\nset +o pipefail\nfalse | true\necho \"exit: $?\"");
        assert_eq!(result.stdout, "exit: 0\n");
        assert_eq!(result.exit_code, 0);

        // pipefail + errexit: pipeline failure triggers errexit
        let result = shell().exec("set -e\nset -o pipefail\necho before\nfalse | true\necho after");
        assert_eq!(result.stdout, "before\n");
        assert_eq!(result.exit_code, 1);

        // errexit without pipefail: pipeline does not trigger errexit
        let result = shell().exec("set -e\necho before\nfalse | true\necho after");
        assert_eq!(result.stdout, "before\nafter\n");
        assert_eq!(result.exit_code, 0);

        // single command pipeline
        let result = shell().exec("set -o pipefail\nfalse\necho \"exit: $?\"");
        assert_eq!(result.stdout, "exit: 1\n");
        assert_eq!(result.exit_code, 0);
    }

    // JBC-13: portable while/until guard conditions mirroring
    // packages/just-bash/src/syntax/loops.test.ts (the two non-grep cases).
    #[test]
    fn jbc13_syntax_loop_guard_conditions_match_upstream() {
        // while loops: should not execute when condition is initially false
        let result = shell().exec("while false; do echo never; done");
        assert_eq!(result.stdout, "");
        assert_eq!(result.exit_code, 0);

        // until loops: should not execute when condition is initially true
        let result = shell().exec("until true; do echo never; done");
        assert_eq!(result.stdout, "");
        assert_eq!(result.exit_code, 0);
    }

    // JBC-13: portable `local` scoping mirroring
    // packages/just-bash/src/syntax/control-flow.test.ts `local keyword` cases.
    #[test]
    fn jbc13_syntax_local_keyword_scopes_match_upstream() {
        // should shadow outer variable
        let result = shell()
            .with_env([("x", "outer")])
            .exec("test_func() { local x=inner; echo $x; }; test_func");
        assert_eq!(result.stdout, "inner\n");

        // should keep local changes within same scope
        let result = shell().exec("test_func() { local x=first; x=second; echo $x; }; test_func");
        assert_eq!(result.stdout, "second\n");
    }

    // JBC-13: portable `!` negation operator mirroring
    // packages/just-bash/src/syntax/control-flow.test.ts `! negation operator` cases.
    #[test]
    fn jbc13_syntax_negation_operator_matches_upstream() {
        // should negate exit code of true to 1
        let result = shell().exec("! true");
        assert_eq!(result.exit_code, 1);

        // should negate exit code of false to 0
        let result = shell().exec("! false");
        assert_eq!(result.exit_code, 0);

        // should work with && chaining
        let result = shell().exec("! false && echo success");
        assert_eq!(result.stdout, "success\n");
        assert_eq!(result.exit_code, 0);

        // should work with || chaining
        let result = shell().exec("! true || echo fallback");
        assert_eq!(result.stdout, "fallback\n");
        assert_eq!(result.exit_code, 0);

        // should work in if condition
        let result = shell().exec("if ! false; then echo yes; fi");
        assert_eq!(result.stdout, "yes\n");
    }

    /// Covers portable operator-precedence rows from
    /// `packages/just-bash/src/syntax/subshell-args.test.ts` (the
    /// "Operator precedence" describe block) through the Rust shell. `!` binds
    /// tighter than `&&`/`||`, negates the whole pipeline, `&&`/`||`/`;` keep
    /// bash precedence/associativity, and stacked `!` toggles the exit status.
    /// The positional-argument (`bash -c`, `sh -c`, script-file) and `xargs`
    /// rows require subshell-arg and command-family behavior the fake command
    /// dispatcher does not model, so they stay pending with command owners.
    #[test]
    fn jb_subshell_args_operator_precedence_rows_match_upstream() {
        for (source, expected_stdout, expected_exit) in [
            // L80 `!` binds tighter than `&&`: `! false` succeeds, then `&& echo yes`.
            ("! false && echo yes", "yes\n", 0),
            // L88 `!` binds tighter than `||`: `! true` fails, then `|| echo fallback`.
            ("! true || echo fallback", "fallback\n", 0),
            // L96 `!` negates the entire pipeline: grep fails (1), negated to 0.
            ("! echo hello | grep missing", "", 0),
            // L105 `!` negates a successful pipeline: grep succeeds (0), negated to 1.
            ("! echo hello | grep hello", "hello\n", 1),
            // L113 `&&`/`||` are left-associative: (true || echo no) && echo yes.
            ("true || echo no && echo yes", "yes\n", 0),
            // L122 `;` has lowest precedence: (false && echo no) ; echo always.
            ("false && echo no ; echo always", "always\n", 0),
            // L130 double negation cancels: ! ! true == 0.
            ("! ! true", "", 0),
            // L137 double negation of false gives 1.
            ("! ! false", "", 1),
            // L144 triple negation negates once: ! ! ! true == 1.
            ("! ! ! true", "", 1),
            // L151 triple negation of false gives 0.
            ("! ! ! false", "", 0),
            // L158 quadruple negation cancels out: ! ! ! ! true == 0.
            ("! ! ! ! true", "", 0),
        ] {
            let result = shell().exec(source);
            assert_eq!(result.stdout, expected_stdout, "stdout {source:?}");
            assert_eq!(result.exit_code, expected_exit, "exit {source:?}");
        }
    }

    /// Covers the remaining portable row from
    /// `packages/just-bash/src/syntax/variables.test.ts` not already mapped by
    /// JBC-33: a literal newline preserved inside double quotes (L131) through
    /// the Rust shell quoting pipeline. The byte-level backslash escape row
    /// (L157) stays pending — like JBC-33's documented exception — because the
    /// Rust `echo -e` backslash collapse diverges on that edge case.
    #[test]
    fn jb_syntax_variables_quoted_newline_row_matches_upstream() {
        // L131 a literal newline inside double quotes is preserved verbatim.
        let result = shell().exec("echo \"line1\nline2\"");
        assert_eq!(result.stdout, "line1\nline2\n");
        assert_eq!(result.exit_code, 0);
    }

    /// Covers portable `while`/`until` guard rows from
    /// `packages/just-bash/src/syntax/loops.test.ts` not already mapped by
    /// JBC-33: condition-driven `while grep -q` loops that flip a guard file,
    /// the no-op `while false`/`until true` loops, and exit-status propagation
    /// from the loop body. The infinite-loop protection rows (L135/L145/L153)
    /// require execution-limit diagnostics, the `until grep -q` guard rows
    /// depend on multi-iteration condition re-evaluation timing, and the
    /// loop-variable cleanup row (L60) needs post-loop variable unsetting, so
    /// those stay pending separately.
    #[test]
    fn jb_syntax_loops_while_guard_rows_match_upstream() {
        // L69 while runs while condition holds, mutating the guard file.
        let mut sh = shell();
        sh.files_mut().write("/count.txt", "0\n");
        let result =
            sh.exec("while grep -q 0 /count.txt; do echo iteration; echo 1 > /count.txt; done");
        assert_eq!(result.stdout, "iteration\n", "L69");

        // L79 while with an initially-false condition runs zero times.
        let result = shell().exec("while false; do echo never; done");
        assert_eq!(result.stdout, "", "L79");
        assert_eq!(result.exit_code, 0, "L79");

        // L86 multiple-iteration guard collapses to one run once flipped.
        let mut sh = shell();
        sh.files_mut().write("/counter.txt", "aaa\n");
        let result = sh.exec(
            "while grep -q aaa /counter.txt; do echo loop; echo \"bbb\" > /counter.txt; done",
        );
        assert_eq!(result.stdout, "loop\n", "L86");

        // L97 exit status comes from the last command in the body (`true`).
        let mut sh = shell();
        sh.files_mut().write("/f.txt", "start\n");
        let result = sh.exec("while grep -q start /f.txt; do echo done > /f.txt; true; done");
        assert_eq!(result.exit_code, 0, "L97");

        // L117 until with an initially-true condition runs zero times.
        let result = shell().exec("until true; do echo never; done");
        assert_eq!(result.stdout, "", "L117");
        assert_eq!(result.exit_code, 0, "L117");

        // L171 simplified for-inside-while case: condition-driven single pass.
        let mut sh = shell();
        sh.files_mut().write("/run.txt", "go\n");
        let result =
            sh.exec("while grep -q go /run.txt; do echo inner; echo stop > /run.txt; done");
        assert_eq!(result.stdout, "inner\n", "L171");

        // L190 while loop with a semicolon before `do` flips the guard file.
        let mut sh = shell();
        sh.files_mut().write("/f.txt", "x\n");
        let result = sh.exec("while grep -q x /f.txt; do echo found; echo y > /f.txt; done");
        assert_eq!(result.stdout, "found\n", "L190");
    }

    /// Covers portable operator rows from
    /// `packages/just-bash/src/syntax/operators.test.ts` not already mapped by
    /// JBC-33: `&&` short-circuit protecting the filesystem, `||` chains over
    /// failing `cat`, `;` exit-status propagation, mixed `&&`/`||`/`;`
    /// precedence chains, and cross-`exec` `>>` appends through the Rust shell.
    /// Rows needing head/tail/wc command families stay pending with command
    /// owners.
    #[test]
    fn jb_syntax_operators_logical_and_redirection_rows_match_upstream() {
        // L43 `&&` short-circuits, so the failing `cat` keeps `rm` from running.
        let mut sh = shell();
        sh.files_mut().write("/important.txt", "keep this");
        sh.exec("cat /missing && rm /important.txt");
        assert_eq!(
            sh.files().read_to_string("/important.txt"),
            Some("keep this"),
            "L43"
        );

        // L91 `||` chain stops at the first success: second `cat` wins.
        let mut sh = shell();
        sh.files_mut().write("/exists.txt", "found");
        let result = sh.exec("cat /missing || cat /exists.txt || echo fallback");
        assert_eq!(result.stdout, "found", "L91");
        assert_eq!(result.exit_code, 0, "L91");

        // The remaining stdout/exit rows only use echo/cat and chain operators.
        for (source, expected_stdout, expected_exit) in [
            // L102 all-failing `||` chain returns the last failure (1).
            ("cat /a || cat /b || cat /c", "", 1),
            // L147 `;` keeps the exit status of the last command (failing cat).
            ("echo first ; cat /missing", "first\n", 1),
            // L154 `;` succeeds when the last command succeeds.
            ("cat /missing ; echo success", "success\n", 0),
            // L185 success && success || fallback runs both successes only.
            ("echo a && echo b || echo c", "a\nb\n", 0),
            // L191 `;` then `&&`: all three run.
            ("echo a ; echo b && echo c", "a\nb\nc\n", 0),
            // L197 `;` then `||`: fallback recovers the failing second cat.
            (
                "cat /missing ; cat /missing2 || echo fallback",
                "fallback\n",
                0,
            ),
            // L205 fail && x || recover ; continue.
            (
                "cat /missing && echo success || echo recovered ; echo done",
                "recovered\ndone\n",
                0,
            ),
            // L275 `||` is not confused with the pipe operator.
            ("cat /missing || echo fallback", "fallback\n", 0),
        ] {
            let result = shell().exec(source);
            assert_eq!(result.stdout, expected_stdout, "stdout {source:?}");
            assert_eq!(result.exit_code, expected_exit, "exit {source:?}");
        }

        // L339 `>>` appends across separate exec calls in the same session.
        let mut sh = shell();
        sh.exec("echo a >> /log.txt");
        sh.exec("echo b >> /log.txt");
        sh.exec("echo c >> /log.txt");
        assert_eq!(
            sh.files().read_to_string("/log.txt"),
            Some("a\nb\nc\n"),
            "L339"
        );
    }

    /// Covers portable `packages/just-bash/src/interpreter/control-flow.test.ts`
    /// rows that the original `r2_interpreter_control_flow_rows_match_upstream`
    /// intentionally deferred: IFS field-splitting in `for ... in`, positional-
    /// parameter iteration (`for i; do`), the invalid-identifier runtime error,
    /// the five C-style `for (( ))` rows, and the `;&` / `;;&` case terminators.
    /// Each block mirrors one upstream `it(...)` assertion on `Bash().exec(...)`.
    #[test]
    fn r10jb_interpreter_control_flow_loops_and_case_modifier_rows_match_upstream() {
        // L113 IFS splitting: IFS=":" splits "a:b:c" into three fields.
        let mut ifs = shell();
        let r = ifs.exec("IFS=:\nitems=\"a:b:c\"\nfor i in $items; do\n  echo $i\ndone");
        assert_eq!(r.stdout, "a\nb\nc\n", "L113 stdout");
        assert_eq!(r.exit_code, 0, "L113 exit");

        // L150 `for i; do` with no list iterates the positional parameters,
        // which upstream seeds via the `@` environment variable.
        let mut pos = shell().with_env([("@", "arg1 arg2 arg3")]);
        let r = pos.exec("for i; do\n  echo $i\ndone");
        assert_eq!(r.stdout, "arg1\narg2\narg3\n", "L150 stdout");
        assert_eq!(r.exit_code, 0, "L150 exit");

        // L174 `for 123 in ...` is a runtime "not a valid identifier" error.
        let mut bad = shell();
        let r = bad.exec("for 123 in a b c; do\n  echo $i\ndone");
        assert!(
            r.stderr.contains("not a valid identifier"),
            "L174 stderr {:?}",
            r.stderr
        );
        assert_eq!(r.exit_code, 1, "L174 exit");

        // C-style `for (( ))` rows L187, L198, L209, L221, L233.
        for (source, expected_stdout) in [
            // L187 basic counting loop.
            ("for ((i=0; i<3; i++)); do\n  echo $i\ndone", "0\n1\n2\n"),
            // L198 complex expressions (descending with compound assignment).
            (
                "for ((i=10; i>=0; i-=3)); do\n  echo $i\ndone",
                "10\n7\n4\n1\n",
            ),
            // L209 empty init reuses the pre-existing variable.
            ("i=0\nfor ((; i<3; i++)); do\n  echo $i\ndone", "0\n1\n2\n"),
            // L221 empty condition is an infinite loop terminated by break.
            (
                "for ((i=0; ; i++)); do\n  echo $i\n  if [ $i -ge 2 ]; then break; fi\ndone",
                "0\n1\n2\n",
            ),
            // L233 `continue` still runs the update clause.
            (
                "for ((i=0; i<5; i++)); do\n  if [ $i -eq 2 ]; then continue; fi\n  echo $i\ndone",
                "0\n1\n3\n4\n",
            ),
        ] {
            let mut sh = shell();
            let result = sh.exec(source);
            assert_eq!(result.stderr, "", "cstyle stderr {source:?}");
            assert_eq!(result.stdout, expected_stdout, "cstyle stdout {source:?}");
            assert_eq!(result.exit_code, 0, "cstyle exit {source:?}");
        }

        // L428 `;&` falls through to the next clause's body unconditionally.
        let mut fall = shell();
        let r = fall.exec(
            "x=a\ncase $x in\n  a) echo \"a\" ;&\n  b) echo \"b\" ;;\n  c) echo \"c\" ;;\nesac",
        );
        assert_eq!(r.stdout, "a\nb\n", "L428 stdout");
        assert_eq!(r.exit_code, 0, "L428 exit");

        // L442 `;;&` continues testing the remaining clause patterns.
        let mut cont = shell();
        let r = cont.exec(
            "x=abc\ncase $x in\n  *a*) echo \"has a\" ;;&\n  *b*) echo \"has b\" ;;&\n  *c*) echo \"has c\" ;;\nesac",
        );
        assert_eq!(r.stdout, "has a\nhas b\nhas c\n", "L442 stdout");
        assert_eq!(r.exit_code, 0, "L442 exit");
    }

    /// Covers the portable arithmetic error rows in
    /// `packages/just-bash/src/interpreter/arithmetic.test.ts` (L333 division by
    /// zero, L340 modulo by zero, L347 negative exponent). Each upstream `it`
    /// asserts the failing expansion reports a diagnostic on stderr and exits 1.
    #[test]
    fn r10jb_interpreter_arithmetic_error_rows_match_upstream() {
        // L333 division by zero.
        let mut sh = shell();
        let r = sh.exec("echo $((5 / 0))");
        assert!(
            r.stderr.contains("division by 0"),
            "L333 stderr {:?}",
            r.stderr
        );
        assert_eq!(r.exit_code, 1, "L333 exit");

        // L340 modulo by zero shares the "division by 0" diagnostic.
        let mut sh = shell();
        let r = sh.exec("echo $((5 % 0))");
        assert!(
            r.stderr.contains("division by 0"),
            "L340 stderr {:?}",
            r.stderr
        );
        assert_eq!(r.exit_code, 1, "L340 exit");

        // L347 negative exponent.
        let mut sh = shell();
        let r = sh.exec("echo $((2 ** -1))");
        assert!(
            r.stderr.contains("exponent less than 0"),
            "L347 stderr {:?}",
            r.stderr
        );
        assert_eq!(r.exit_code, 1, "L347 exit");
    }

    /// Covers portable rows in
    /// `packages/just-bash/src/interpreter/prototype-pollution.test.ts` that the
    /// in-process interpreter handles without external commands: JavaScript
    /// prototype keywords (`constructor`, `__proto__`, `prototype`, ...) are
    /// treated as ordinary bash identifiers, array values/elements, parameter
    /// expansions, comparisons, function/alias/local names, loop conditions,
    /// subshells, and command substitutions. Each tuple mirrors one upstream
    /// `it(...)` stdout/exit-code assertion on `Bash().exec(...)`.
    #[test]
    fn r10jb_interpreter_prototype_pollution_identifier_rows_match_upstream() {
        for (line, source, expected_stdout) in [
            // L135 prototype keywords stored as array values round-trip.
            (
                "L135",
                "arr=(constructor __proto__ prototype); echo ${arr[@]}",
                "constructor __proto__ prototype\n",
            ),
            // L228 `[[ ... == ... ]]` compares keyword strings literally.
            (
                "L228",
                "if [[ \"constructor\" == \"constructor\" ]]; then echo yes; else echo no; fi",
                "yes\n",
            ),
            // L349 a variable named like a prototype keyword assigns/reads.
            (
                "L349",
                "constructor=test_value; echo $constructor",
                "test_value\n",
            ),
            // L362 a function named `constructor` defines and runs.
            (
                "L362",
                "constructor() { echo 'called constructor'; }; constructor",
                "called constructor\n",
            ),
            // L375 an alias named `constructor` expands and runs.
            (
                "L375",
                "shopt -s expand_aliases; alias constructor='echo aliased'; constructor",
                "aliased\n",
            ),
            // L388 a local variable named `constructor` is scoped correctly.
            (
                "L388",
                "testfunc() {\n  local constructor=local_value\n  echo $constructor\n}\ntestfunc",
                "local_value\n",
            ),
            // L451 an array of all dangerous keywords round-trips.
            (
                "L451",
                "arr=(constructor __proto__ prototype hasOwnProperty isPrototypeOf); echo ${arr[@]}",
                "constructor __proto__ prototype hasOwnProperty isPrototypeOf\n",
            ),
            // L459 an indexed array named `__proto__` expands with `[@]`.
            ("L459", "__proto__=(a b c); echo ${__proto__[@]}", "a b c\n"),
            // L466 an indexed array named `constructor` reads element `[1]`.
            ("L466", "constructor=(1 2 3); echo ${constructor[1]}", "2\n"),
            // L655 a keyword-named scalar expands inside a double-quoted string.
            (
                "L655",
                "__proto__=value\necho \"before: $__proto__\"",
                "before: value\n",
            ),
            // L665 a keyword-named scalar expands unquoted.
            ("L665", "__proto__=test\necho $__proto__", "test\n"),
            // L678 `REPLY` may hold a keyword value.
            (
                "L678",
                "REPLY=__proto__\necho \"REPLY: $REPLY\"",
                "REPLY: __proto__\n",
            ),
            // L688 `PS3` may contain keyword text.
            (
                "L688",
                "PS3=\"__proto__> \"\necho \"PS3: $PS3\"",
                "PS3: __proto__> \n",
            ),
            // L868 `${#var}` length works on a keyword-named variable.
            ("L868", "__proto__=12345\necho ${#__proto__}", "5\n"),
            // L892 a keyword-named variable drives a `(( ))` while condition.
            (
                "L892",
                "__proto__=3\nwhile (( __proto__ > 0 )); do\n  echo $__proto__\n  ((__proto__--))\ndone",
                "3\n2\n1\n",
            ),
            // L903 a keyword-named variable set inside a subshell.
            (
                "L903",
                "( __proto__=subshell; echo $__proto__ )",
                "subshell\n",
            ),
            // L916 command substitution yielding keyword text.
            (
                "L916",
                "result=$(echo __proto__)\necho \"got: $result\"",
                "got: __proto__\n",
            ),
        ] {
            let mut sh = shell();
            let result = sh.exec(source);
            assert_eq!(result.stdout, expected_stdout, "{line} stdout {source:?}");
            assert_eq!(result.exit_code, 0, "{line} exit {source:?}");
        }
    }

    /// JBC-50 closes the upstream `local` builtin rows
    /// (packages/just-bash/src/interpreter/builtins/local.test.ts) against the
    /// portable Rust shell interpreter. Each row exercises function-scoped
    /// declaration, shadowing, restoration after return, multiple/mixed
    /// declarations, nested and recursive scoping, special values, variable
    /// expansion on the RHS, and the outside-function / subshell error paths.
    /// Every assertion fails if the dynamic-scope semantics of `local` regress.
    #[test]
    fn jbpi_local_builtin_function_scoping_rows_match_upstream() {
        // basic local variables.
        // local.test.ts:6 declare local variable with value.
        let mut sh = shell();
        assert_eq!(
            sh.exec("test_func() { local x=hello; echo $x; }; test_func")
                .stdout,
            "hello\n"
        );
        // local.test.ts:14 should not affect outer scope.
        let mut sh = shell().with_env([("x", "outer")]);
        assert_eq!(
            sh.exec("test_func() { local x=inner; echo $x; }; test_func; echo $x")
                .stdout,
            "inner\nouter\n"
        );
        // local.test.ts:22 should shadow outer variable.
        let mut sh = shell().with_env([("x", "outer")]);
        assert_eq!(
            sh.exec("test_func() { local x=inner; echo $x; }; test_func")
                .stdout,
            "inner\n"
        );
        // local.test.ts:30 should restore undefined variable after function.
        let mut sh = shell();
        assert_eq!(
            sh.exec(
                "test_func() { local newvar=value; echo $newvar; }; test_func; echo \"[$newvar]\""
            )
            .stdout,
            "value\n[]\n"
        );
        // local.test.ts:38 should declare local without value.
        let mut sh = shell();
        assert_eq!(
            sh.exec("test_func() { local x; x=assigned; echo $x; }; test_func")
                .stdout,
            "assigned\n"
        );

        // multiple local declarations.
        // local.test.ts:48 multiple local declarations.
        let mut sh = shell();
        assert_eq!(
            sh.exec("test_func() { local a=1 b=2 c=3; echo $a $b $c; }; test_func")
                .stdout,
            "1 2 3\n"
        );
        // local.test.ts:56 mixed declarations with and without values.
        let mut sh = shell();
        assert_eq!(
            sh.exec("test_func() { local a=1 b c=3; b=2; echo $a $b $c; }; test_func")
                .stdout,
            "1 2 3\n"
        );

        // nested functions.
        // local.test.ts:66 nested function calls.
        let mut sh = shell();
        assert_eq!(
            sh.exec("inner() { local x=inner; echo $x; }; outer() { local x=outer; inner; echo $x; }; outer")
                .stdout,
            "inner\nouter\n"
        );
        // local.test.ts:74 keep local changes within same scope.
        let mut sh = shell();
        assert_eq!(
            sh.exec("test_func() { local x=first; x=second; echo $x; }; test_func")
                .stdout,
            "second\n"
        );
        // local.test.ts:82 not leak local from inner to outer function.
        let mut sh = shell();
        assert_eq!(
            sh.exec(
                "\
                inner() { local y=inner; }\n\
                outer() {\n\
                  local x=outer\n\
                  inner\n\
                  echo \"x=$x y=$y\"\n\
                }\n\
                outer\n"
            )
            .stdout,
            "x=outer y=\n"
        );

        // error cases.
        // local.test.ts:98 error when used outside function.
        let mut sh = shell();
        let outside = sh.exec("local x=value");
        assert_eq!(outside.exit_code, 1, "outside-function exit");
        assert!(
            outside.stderr.contains("can only be used in a function"),
            "stderr was: {}",
            outside.stderr
        );
        // local.test.ts:105 error when used in subshell outside function.
        let mut sh = shell();
        assert_ne!(sh.exec("(local x=value)").exit_code, 0);

        // local with special values.
        // local.test.ts:113 local with empty value.
        let mut sh = shell();
        assert_eq!(
            sh.exec("test_func() { local x=; echo \"x is $x end\"; }; test_func")
                .stdout,
            "x is  end\n"
        );
        // local.test.ts:121 local with spaces in value (quoted).
        let mut sh = shell();
        assert_eq!(
            sh.exec("test_func() { local x=\"hello world\"; echo \"$x\"; }; test_func")
                .stdout,
            "hello world\n"
        );
        // local.test.ts:129 local with variable expansion.
        let mut sh = shell().with_env([("OUTER", "expanded")]);
        assert_eq!(
            sh.exec("test_func() { local x=$OUTER; echo \"$x\"; }; test_func")
                .stdout,
            "expanded\n"
        );

        // local scope restoration.
        // local.test.ts:139 restore original value after function returns.
        let mut sh = shell();
        assert_eq!(
            sh.exec(
                "\
                x=global\n\
                test_func() {\n\
                  local x=local\n\
                  echo \"inside: $x\"\n\
                }\n\
                echo \"before: $x\"\n\
                test_func\n\
                echo \"after: $x\"\n"
            )
            .stdout,
            "before: global\ninside: local\nafter: global\n"
        );
        // local.test.ts:156 recursive functions with local.
        let mut sh = shell();
        assert_eq!(
            sh.exec(
                "\
                countdown() {\n\
                  local n=$1\n\
                  if [ $n -le 0 ]; then\n\
                    echo \"done\"\n\
                    return\n\
                  fi\n\
                  echo $n\n\
                  countdown $((n - 1))\n\
                }\n\
                countdown 3\n"
            )
            .stdout,
            "3\n2\n1\ndone\n"
        );
    }

    /// JBC-50 closes additional portable parser/interpreter rows across the
    /// `return`, `continue`, `eval`, and function/control-flow suites that run
    /// in the FakeCommands harness:
    ///   - return.test.ts:121,141,161,175 (innermost-only return, propagation
    ///     through loops/if, stdout/stderr preserved before return).
    ///   - continue.test.ts:197,208 (continue in / update-after-continue for a
    ///     C-style for loop).
    ///   - eval.test.ts:154 (escaped characters within eval).
    ///   - control-flow.test.ts:202 (functions do not persist across exec).
    /// Each assertion fails if the corresponding control-flow semantics regress.
    #[test]
    fn jbpi_return_continue_eval_and_function_persistence_rows_match_upstream() {
        // return.test.ts:121 only return from innermost function.
        let mut sh = shell();
        assert_eq!(
            sh.exec(
                "\
                outer() {\n\
                  echo outer-start\n\
                  inner() {\n\
                    echo inner\n\
                    return 5\n\
                  }\n\
                  inner\n\
                  echo \"inner returned $?\"\n\
                }\n\
                outer\n\
                echo \"outer returned $?\"\n"
            )
            .stdout,
            "outer-start\ninner\ninner returned 5\nouter returned 0\n"
        );

        // return.test.ts:141 propagate return through control flow.
        let mut sh = shell();
        assert_eq!(
            sh.exec(
                "\
                myfunc() {\n\
                  for i in 1 2 3; do\n\
                    if [ $i -eq 2 ]; then\n\
                      return 42\n\
                    fi\n\
                    echo $i\n\
                  done\n\
                  echo \"never\"\n\
                }\n\
                myfunc\n\
                echo $?\n"
            )
            .stdout,
            "1\n42\n"
        );

        // return.test.ts:161 preserve stdout before return.
        let mut sh = shell();
        assert_eq!(
            sh.exec(
                "\
                myfunc() {\n\
                  echo line1\n\
                  echo line2\n\
                  return 3\n\
                }\n\
                myfunc\n\
                echo \"exit: $?\"\n"
            )
            .stdout,
            "line1\nline2\nexit: 3\n"
        );

        // return.test.ts:175 preserve stderr/exit before return (command not found).
        let mut sh = shell();
        let r = sh.exec(
            "\
            myfunc() {\n\
              nonexistent_cmd_xyz 2>/dev/null || true\n\
              return 5\n\
            }\n\
            myfunc\n",
        );
        assert_eq!(r.exit_code, 5);

        // continue.test.ts:197 continue in C-style for loop.
        let mut sh = shell();
        assert_eq!(
            sh.exec(
                "\
                for ((i=1; i<=5; i++)); do\n\
                  if [ $i -eq 3 ]; then continue; fi\n\
                  echo $i\n\
                done\n"
            )
            .stdout,
            "1\n2\n4\n5\n"
        );

        // continue.test.ts:208 update expression runs after continue.
        let mut sh = shell();
        assert_eq!(
            sh.exec(
                "\
                for ((i=0; i<5; i++)); do\n\
                  if [ $i -lt 3 ]; then continue; fi\n\
                  echo $i\n\
                done\n"
            )
            .stdout,
            "3\n4\n"
        );

        // eval.test.ts:154 escaped characters within eval (output contains hello).
        let mut sh = shell();
        let eval_escape = sh.exec("eval \"echo hello\\\\nworld\"");
        assert!(
            eval_escape.stdout.contains("hello"),
            "eval escape stdout: {:?}",
            eval_escape.stdout
        );

        // control-flow.test.ts:202 function definitions do not persist across exec.
        let mut sh = shell();
        sh.exec("greet() { echo hello; }");
        assert_eq!(sh.exec("greet").exit_code, 127);
    }

    /// Maps `packages/just-bash/src/syntax/loops.test.ts` loop-protection rows
    /// (`:135` infinite for, `:145` infinite while, `:153` infinite until). Each
    /// runaway loop must be bounded by the interpreter's execution limits and
    /// surface a "too many iterations/commands" diagnostic instead of hanging.
    #[test]
    fn jbpi_syntax_loops_protection_rows_match_upstream() {
        // :135 a 10001-element for-list is rejected by the iteration/command cap.
        let long_list = vec!["x"; 10001].join(" ");
        let for_loop = shell().exec(&format!("for i in {long_list}; do echo $i; done"));
        assert_ne!(for_loop.exit_code, 0, "for stderr={:?}", for_loop.stderr);
        assert!(
            for_loop.stderr.contains("too many iterations")
                || for_loop.stderr.contains("too many commands"),
            "for stderr={:?}",
            for_loop.stderr
        );
        // :145 `while true` is bounded by the same protection.
        let while_loop = shell().exec("while true; do echo loop; done");
        assert_ne!(
            while_loop.exit_code, 0,
            "while stderr={:?}",
            while_loop.stderr
        );
        assert!(
            while_loop.stderr.contains("too many iterations")
                || while_loop.stderr.contains("too many commands"),
            "while stderr={:?}",
            while_loop.stderr
        );
        // :153 `until false` is bounded by the same protection.
        let until_loop = shell().exec("until false; do echo loop; done");
        assert_ne!(
            until_loop.exit_code, 0,
            "until stderr={:?}",
            until_loop.stderr
        );
        assert!(
            until_loop.stderr.contains("too many iterations")
                || until_loop.stderr.contains("too many commands"),
            "until stderr={:?}",
            until_loop.stderr
        );
    }

    /// Maps `packages/just-bash/src/syntax/execution-protection.test.ts`. The
    /// interpreter must bound function recursion depth (`maxCallDepth`), total
    /// command count (`maxCommandCount`), and loop iterations (`maxLoopIterations`)
    /// with the upstream `ExecutionLimitError` exit code (126) and diagnostic
    /// strings, regardless of whether the runaway is reached through plain
    /// recursion, mutual recursion, `eval`, command substitution, arithmetic
    /// expansion, subshells, pipelines, `case`, or `local`. Each hard assertion
    /// below fails if a protection gap regresses (hang / stack overflow / wrong
    /// exit code / missing message).
    #[test]
    fn jbpi_syntax_execution_protection_rows_match_upstream() {
        fn shell_with(limits: ExecutionLimits) -> Interpreter<FakeCommands> {
            Interpreter::new(FakeCommands::default()).with_limits(limits)
        }
        fn limits(max_call_depth: usize) -> ExecutionLimits {
            ExecutionLimits {
                max_call_depth,
                ..ExecutionLimits::default()
            }
        }
        // Shared assertion mirroring upstream `expectProtectionTriggered`: a
        // safety limit triggered (exit 126), produced a diagnostic, and is NOT a
        // native stack overflow (our limits kick in first).
        fn expect_protection_triggered(label: &str, output: &ExecOutput) {
            assert_eq!(
                output.exit_code, EXECUTION_LIMIT_EXIT_CODE,
                "{label}: expected execution-limit exit code, stderr={:?}",
                output.stderr
            );
            assert!(
                !output.stderr.is_empty(),
                "{label}: expected a diagnostic message"
            );
            assert!(
                !output.stderr.contains("stack"),
                "{label}: must not be a native stack overflow, stderr={:?}",
                output.stderr
            );
        }

        // recursion depth protection ---------------------------------------
        // :35 simple infinite recursion errors with the recursion diagnostic.
        let result = shell_with(limits(50)).exec("recurse() { recurse; }; recurse");
        assert_eq!(result.exit_code, EXECUTION_LIMIT_EXIT_CODE);
        assert!(result.stderr.contains("maximum recursion depth"));
        assert!(result.stderr.contains("exceeded"));

        // :44 reasonable bounded recursion (countdown) succeeds.
        let countdown = shell().exec(
            "echo 5 > /count.txt; countdown() { local n=$(cat /count.txt); \
             if [ \"$n\" -gt 0 ]; then echo $n; echo $((n-1)) > /count.txt; countdown; fi; }; \
             countdown",
        );
        assert_eq!(
            countdown.exit_code, 0,
            "countdown stderr={:?}",
            countdown.stderr
        );

        // :52 recursion error names the offending function.
        let named = shell_with(limits(50)).exec("myinfinite() { myinfinite; }; myinfinite");
        assert!(named.stderr.contains("myinfinite"));
        assert!(named.stderr.contains("maximum recursion depth"));

        // :60 mutual recursion (A->B->A) is bounded with the recursion message.
        let mutual = shell_with(limits(50)).exec("ping() { pong; }\npong() { ping; }\nping\n");
        expect_protection_triggered("mutual recursion", &mutual);
        assert!(mutual.stderr.contains("maximum recursion depth"));

        // :72 three-way mutual recursion (A->B->C->A) is bounded.
        let three_way = shell_with(limits(50)).exec("a() { b; }\nb() { c; }\nc() { a; }\na\n");
        expect_protection_triggered("three-way mutual recursion", &three_way);

        // :84 recursion through eval is bounded.
        let via_eval = shell_with(limits(50)).exec("boom() { eval 'boom'; }\nboom\n");
        expect_protection_triggered("recursion through eval", &via_eval);

        // :94 recursion through command substitution is bounded.
        let via_subst = shell_with(limits(50)).exec("boom() { echo $(boom); }\nboom\n");
        expect_protection_triggered("recursion through command substitution", &via_subst);

        // :104 recursion carrying local variables is bounded.
        let via_local = shell_with(limits(50)).exec(
            "deep() {\n  local depth=$1\n  echo \"depth: $depth\"\n  deep $((depth + 1))\n}\ndeep 0\n",
        );
        expect_protection_triggered("recursion with local variables", &via_local);

        // :118 recursion through arithmetic expansion is bounded.
        let via_arith =
            shell_with(limits(50)).exec("counter=0\nboom() { echo $((counter++)); boom; }\nboom\n");
        expect_protection_triggered("recursion through arithmetic expansion", &via_arith);

        // command count protection -----------------------------------------
        // :131 `while true` is bounded by the loop iteration cap.
        let too_many_iter = shell_with(ExecutionLimits {
            max_loop_iterations: 100,
            ..ExecutionLimits::default()
        })
        .exec("while true; do echo x; done");
        assert_eq!(too_many_iter.exit_code, EXECUTION_LIMIT_EXIT_CODE);
        assert!(too_many_iter.stderr.contains("too many iterations"));

        // :139 the command count resets between exec calls.
        let mut session = shell();
        session.exec("echo 1; echo 2; echo 3");
        let reset = session.exec("echo done");
        assert_eq!(reset.stdout, "done\n");
        assert_eq!(reset.exit_code, 0);

        // :147 200 semicolon-separated commands trip a 100-command cap.
        let commands = vec!["echo x"; 200].join("; ");
        let many_semis = shell_with(ExecutionLimits {
            max_commands: 100,
            ..ExecutionLimits::default()
        })
        .exec(&commands);
        expect_protection_triggered("many commands via semicolons", &many_semis);
        assert!(many_semis.stderr.contains("too many commands"));

        // :157 the fork-bomb pattern is bounded by recursion/command limits.
        let fork_bomb = shell_with(ExecutionLimits {
            max_call_depth: 20,
            max_commands: 1000,
            ..ExecutionLimits::default()
        })
        .exec("bomb() { bomb | bomb & }\nbomb\n");
        expect_protection_triggered("fork bomb pattern", &fork_bomb);

        // loop protection ---------------------------------------------------
        // :180 `while true` is bounded.
        let inf_while = shell_with(ExecutionLimits {
            max_loop_iterations: 100,
            ..ExecutionLimits::default()
        })
        .exec("while true; do echo loop; done");
        assert_eq!(inf_while.exit_code, EXECUTION_LIMIT_EXIT_CODE);
        assert!(inf_while.stderr.contains("too many iterations"));

        // :196 nested infinite loops are bounded.
        let nested = shell_with(ExecutionLimits {
            max_loop_iterations: 100,
            ..ExecutionLimits::default()
        })
        .exec("while true; do\n  while true; do\n    echo inner\n  done\ndone\n");
        expect_protection_triggered("nested infinite loops", &nested);

        // :216 an infinite loop whose break never triggers is bounded.
        let never_break = shell_with(ExecutionLimits {
            max_loop_iterations: 100,
            ..ExecutionLimits::default()
        })
        .exec("while true; do\n  if false; then break; fi\n  echo loop\ndone\n");
        expect_protection_triggered("break that never triggers", &never_break);

        // :228 continue abuse inside an infinite loop is bounded.
        let continue_abuse = shell_with(ExecutionLimits {
            max_loop_iterations: 100,
            ..ExecutionLimits::default()
        })
        .exec("i=0\nwhile true; do\n  i=$((i+1))\n  continue\ndone\n");
        expect_protection_triggered("continue abuse", &continue_abuse);

        // :264 `eval` inside an infinite loop is bounded.
        let eval_in_loop = shell_with(ExecutionLimits {
            max_loop_iterations: 100,
            ..ExecutionLimits::default()
        })
        .exec("while true; do\n  eval 'echo x'\ndone\n");
        expect_protection_triggered("eval in loop", &eval_in_loop);

        // loop protection ---------------------------------------------------
        // :171 a 200-element for-list exceeds a 100-iteration cap.
        let long_list = vec!["x"; 200].join(" ");
        let big_for = shell_with(ExecutionLimits {
            max_loop_iterations: 100,
            ..ExecutionLimits::default()
        })
        .exec(&format!("for i in {long_list}; do echo $i; done"));
        assert_eq!(big_for.exit_code, EXECUTION_LIMIT_EXIT_CODE);
        assert!(big_for.stderr.contains("too many iterations"));

        // :188 `until false` is bounded.
        let until_loop = shell_with(ExecutionLimits {
            max_loop_iterations: 100,
            ..ExecutionLimits::default()
        })
        .exec("until false; do echo loop; done");
        assert_eq!(until_loop.exit_code, EXECUTION_LIMIT_EXIT_CODE);
        assert!(until_loop.stderr.contains("too many iterations"));

        // :209 C-style `for ((;;))` infinite loop is bounded.
        let c_style = shell_with(ExecutionLimits {
            max_loop_iterations: 100,
            ..ExecutionLimits::default()
        })
        .exec("for ((;;)); do echo x; done");
        expect_protection_triggered("C-style infinite loop", &c_style);

        // combined protection ----------------------------------------------
        // :252 a loop calling an infinitely recursive function is bounded.
        let loop_calls_recurse = shell_with(ExecutionLimits {
            max_call_depth: 20,
            max_loop_iterations: 100,
            ..ExecutionLimits::default()
        })
        .exec("recurse() { recurse; }\nfor i in 1 2 3 4 5; do\n  recurse\ndone\n");
        expect_protection_triggered("loop calling recursive function", &loop_calls_recurse);

        // expansion protection ---------------------------------------------
        // :349 recursive command substitution via a function is bounded.
        let recursive_subst = shell_with(limits(50)).exec("f() { echo \"$(f)\"; }\nf\n");
        expect_protection_triggered(
            "recursive command substitution via function",
            &recursive_subst,
        );

        // subshell protection ----------------------------------------------
        // :408 infinite subshell recursion is bounded.
        let subshell_recurse = shell_with(ExecutionLimits {
            max_call_depth: 50,
            max_commands: 1000,
            ..ExecutionLimits::default()
        })
        .exec("f() { (f); }\nf\n");
        expect_protection_triggered("infinite subshell recursion", &subshell_recurse);

        // pipeline protection ----------------------------------------------
        // :431 an infinite pipeline through a function is bounded.
        let inf_pipe = shell_with(limits(50))
            .exec("infinite_pipe() { echo x | infinite_pipe; }\ninfinite_pipe\n");
        expect_protection_triggered("infinite pipeline through function", &inf_pipe);

        // edge cases --------------------------------------------------------
        // :529 an empty loop body is still bounded.
        let empty_body = shell_with(ExecutionLimits {
            max_loop_iterations: 100,
            ..ExecutionLimits::default()
        })
        .exec("while true; do :; done");
        expect_protection_triggered("empty loop body", &empty_body);

        // :548 infinite recursion through `case` is bounded.
        let case_recurse =
            shell_with(limits(50)).exec("f() {\n  case x in\n    *) f ;;\n  esac\n}\nf\n");
        expect_protection_triggered("infinite case recursion", &case_recurse);

        // configurable limits ----------------------------------------------
        // :479 a custom maxCallDepth is reflected in the diagnostic.
        let custom_depth = shell_with(limits(5)).exec("recurse() { recurse; }; recurse");
        assert_eq!(custom_depth.exit_code, EXECUTION_LIMIT_EXIT_CODE);
        assert!(custom_depth.stderr.contains("(5)"));
        assert!(custom_depth.stderr.contains("maxCallDepth"));

        // :488 a custom maxLoopIterations is reflected in the diagnostic.
        let custom_iter = shell_with(ExecutionLimits {
            max_loop_iterations: 50,
            ..ExecutionLimits::default()
        })
        .exec("while true; do echo x; done");
        assert_eq!(custom_iter.exit_code, EXECUTION_LIMIT_EXIT_CODE);
        assert!(custom_iter.stderr.contains("(50)"));
        assert!(custom_iter.stderr.contains("maxLoopIterations"));

        // :505 higher loop limits let a 150-element list complete.
        let mut higher = String::from("for i in");
        for _ in 0..150 {
            higher.push_str(" x");
        }
        higher.push_str("; do echo $i; done");
        let higher_ok = shell_with(ExecutionLimits {
            max_loop_iterations: 200,
            ..ExecutionLimits::default()
        })
        .exec(&higher);
        assert_eq!(
            higher_ok.exit_code, 0,
            "higher stderr={:?}",
            higher_ok.stderr
        );

        // :515 very strict limits trip on even a single recursion.
        let strict = shell_with(ExecutionLimits {
            max_call_depth: 3,
            max_loop_iterations: 5,
            max_commands: 10,
        })
        .exec("f() { f; }; f");
        expect_protection_triggered("very strict limits", &strict);

        // input size protection --------------------------------------------
        // :385 input above the parser limit is rejected with a "too large" error.
        let too_long = format!("echo \"{}\"", "x".repeat(1_100_000));
        let oversized = shell().exec(&too_long);
        assert_ne!(oversized.exit_code, 0);
        assert!(
            oversized.stderr.contains("too large"),
            "oversized stderr={:?}",
            oversized.stderr
        );

        // combined protection ----------------------------------------------
        // :243 a recursive function whose body contains a loop is bounded by the
        // call-depth limit (it re-enters itself before the loop can finish).
        let recurse_with_loop = shell_with(ExecutionLimits {
            max_call_depth: 20,
            max_loop_iterations: 100,
            ..ExecutionLimits::default()
        })
        .exec("dangerous() { for i in 1 2 3; do dangerous; done; }; dangerous");
        expect_protection_triggered("recursive function with loops", &recurse_with_loop);

        // brace/range expansion protection ---------------------------------
        // These all exit cleanly (0) with bounded output rather than hanging.
        // :287 a massive numeric range expands without hanging.
        let massive_brace = shell().exec("echo {1..100000}");
        assert_eq!(
            massive_brace.exit_code, 0,
            "massive brace stderr={:?}",
            massive_brace.stderr
        );

        // :296 a wide cartesian brace product completes cleanly.
        let nested_brace =
            shell().exec("echo {a,b}{c,d}{e,f}{g,h}{i,j}{k,l}{m,n}{o,p}{q,r}{s,t}{u,v}{w,x}");
        assert_eq!(
            nested_brace.exit_code, 0,
            "nested brace stderr={:?}",
            nested_brace.stderr
        );

        // :308 a deeply nested brace product completes cleanly.
        let deep_brace =
            shell().exec("echo {a,b,c,d,e}{1,2,3,4,5}{a,b,c,d,e}{1,2,3,4,5}{a,b,c,d,e}");
        assert_eq!(
            deep_brace.exit_code, 0,
            "deep brace stderr={:?}",
            deep_brace.stderr
        );

        // :319 a range with a huge implied step count is bounded, exits cleanly.
        let huge_range = shell().exec("echo {1..1000000..1}");
        assert_eq!(
            huge_range.exit_code, 0,
            "huge range stderr={:?}",
            huge_range.stderr
        );

        // :327 a character range product is bounded, exits cleanly.
        let char_range = shell().exec("echo {a..z}{a..z}{a..z}{a..z}");
        assert_eq!(
            char_range.exit_code, 0,
            "char range stderr={:?}",
            char_range.stderr
        );

        // expansion protection ---------------------------------------------
        // :337 moderately nested command substitution succeeds (not too deep).
        let nested_subst =
            shell_with(limits(50)).exec("echo $(echo $(echo $(echo $(echo $(echo $(echo hi))))))");
        assert_eq!(
            nested_subst.exit_code, 0,
            "nested subst stderr={:?}",
            nested_subst.stderr
        );
        assert_eq!(nested_subst.stdout.trim(), "hi");

        // input size protection --------------------------------------------
        // :396 many separate arguments (1000 tokens) are handled fine.
        let many_args = vec!["arg"; 1000].join(" ");
        let many_args_ok = shell().exec(&format!("echo {many_args}"));
        assert_eq!(
            many_args_ok.exit_code, 0,
            "many args stderr={:?}",
            many_args_ok.stderr
        );

        // subshell protection ----------------------------------------------
        // :418 a subshell spawned inside an infinite loop is bounded.
        let subshell_in_loop = shell_with(ExecutionLimits {
            max_loop_iterations: 100,
            ..ExecutionLimits::default()
        })
        .exec("while true; do\n  (echo nested)\ndone\n");
        expect_protection_triggered("nested subshells in loop", &subshell_in_loop);

        // pipeline protection ----------------------------------------------
        // :441 a long but finite pipeline (50 stages) completes cleanly.
        let long_pipe = vec!["cat"; 50].join(" | ");
        let long_pipe_ok = shell().exec(&format!("echo test | {long_pipe}"));
        assert_eq!(
            long_pipe_ok.exit_code, 0,
            "long pipeline stderr={:?}",
            long_pipe_ok.stderr
        );
        assert_eq!(long_pipe_ok.stdout.trim(), "test");

        // special variable expansion protection ----------------------------
        // :453 setting PROMPT_COMMAND in non-interactive mode is safe (not run).
        let prompt_cmd = shell_with(limits(50)).exec("PROMPT_COMMAND='echo prompt'\necho done\n");
        assert_eq!(
            prompt_cmd.exit_code, 0,
            "PROMPT_COMMAND stderr={:?}",
            prompt_cmd.stderr
        );

        // :465 a self-referential variable is evaluated once, not recursively.
        let self_ref = shell().exec("x='$x'\necho \"$x\"\n");
        assert_eq!(
            self_ref.exit_code, 0,
            "self-ref stderr={:?}",
            self_ref.stderr
        );
        assert_eq!(self_ref.stdout.trim(), "$x");

        // configurable limits ----------------------------------------------
        // :497 a custom maxCommandCount bounds 100 semicolon-separated commands.
        let custom_cmd_count = shell_with(ExecutionLimits {
            max_commands: 50,
            ..ExecutionLimits::default()
        })
        .exec(&vec!["echo x"; 100].join("; "));
        expect_protection_triggered("custom command count", &custom_cmd_count);

        // edge cases --------------------------------------------------------
        // :536 a loop whose body is only a comment plus `:` is still bounded.
        let comment_body = shell_with(ExecutionLimits {
            max_loop_iterations: 100,
            ..ExecutionLimits::default()
        })
        .exec("while true; do\n  # just a comment\n  :\ndone\n");
        expect_protection_triggered("loop with only comments", &comment_body);

        // :562 a simulated `select` loop (while-true) is bounded.
        let select_sim = shell_with(ExecutionLimits {
            max_loop_iterations: 100,
            ..ExecutionLimits::default()
        })
        .exec("PS3='Choose: '\ni=0\nwhile true; do\n  i=$((i+1))\n  echo \"iteration $i\"\ndone\n");
        expect_protection_triggered("simulated select loop", &select_sim);

        // :577 a trap registered before an infinite loop does not defeat bounding.
        let trap_loop = shell_with(ExecutionLimits {
            max_loop_iterations: 100,
            ..ExecutionLimits::default()
        })
        .exec("trap 'echo trapped' EXIT\nwhile true; do echo x; done\n");
        expect_protection_triggered("trap in infinite loop", &trap_loop);

        // performance — these must also resolve quickly, but we only assert the
        // protection outcome here (the timing assertion is JS harness-specific).
        // :589 obvious infinite recursion is rejected.
        let quick_recurse = shell_with(limits(10)).exec("f() { f; }; f");
        expect_protection_triggered("quick infinite recursion", &quick_recurse);

        // :598 an infinite loop is rejected.
        let quick_loop = shell_with(ExecutionLimits {
            max_loop_iterations: 100,
            ..ExecutionLimits::default()
        })
        .exec("while true; do :; done");
        expect_protection_triggered("quick infinite loop", &quick_loop);

        // :607 a massive brace expansion resolves cleanly (no hang).
        let quick_brace = shell().exec("echo {1..100000}");
        assert_eq!(
            quick_brace.exit_code, 0,
            "quick brace stderr={:?}",
            quick_brace.stderr
        );

        // :616 deep mutual recursion is rejected.
        let quick_mutual = shell_with(limits(20)).exec("a() { b; }; b() { c; }; c() { a; }; a");
        expect_protection_triggered("quick deep mutual recursion", &quick_mutual);
    }

    /// Maps `packages/just-bash/src/syntax/parser-protection.test.ts`. The parser
    /// must reject oversized input (`:17`) and complete in bounded time on every
    /// pathological shape (long names/strings, deep nesting, many tokens, etc.),
    /// and execution must bound brace/range expansion (`:209`, `:220`). The hard
    /// failures here are: oversized input throws, each shape parses fast, and the
    /// expansion rows exit cleanly with bounded output.
    #[test]
    fn jbpi_syntax_parser_protection_rows_match_upstream() {
        use std::time::Instant;

        // :17 input above MAX_INPUT_SIZE is rejected before tokenizing.
        let too_long = format!("echo {}", "x".repeat(2_000_000));
        assert!(parse(&too_long).is_err(), ":17 oversized input must error");

        // Each pathological shape must parse (or error) well within its upstream
        // time budget; we assert a generous 5s ceiling so the test is a real
        // anti-hang guard without being flaky on slow CI.
        let bounded = |label: &str, input: &str| {
            let start = Instant::now();
            let _ = parse(input);
            let elapsed = start.elapsed();
            assert!(
                elapsed.as_secs() < 5,
                "{label} took {}ms (parser hang)",
                elapsed.as_millis()
            );
        };
        bounded(
            ":24 long var name",
            &format!("{}=value", "a".repeat(100_000)),
        );
        bounded(
            ":39 long string",
            &format!("echo \"{}\"", "x".repeat(500_000)),
        );
        bounded(
            ":55 nested parens",
            &format!("echo {}test{}", "(".repeat(1000), ")".repeat(1000)),
        );
        bounded(
            ":71 nested braces",
            &format!("echo {}test{}", "{".repeat(1000), "}".repeat(1000)),
        );
        let mut nested_subst = "echo x".to_string();
        for _ in 0..100 {
            nested_subst = format!("echo $({nested_subst})");
        }
        bounded(":87 nested command subst", &nested_subst);
        let mut nested_arith = "1".to_string();
        for _ in 0..500 {
            nested_arith = format!("({nested_arith}+1)");
        }
        bounded(
            ":103 nested arithmetic",
            &format!("echo $(({nested_arith}))"),
        );
        bounded(
            ":122 many tokens",
            &format!("echo {}", vec!["x"; 50000].join(" ")),
        );
        bounded(":137 many semicolons", &vec!["echo x"; 10000].join("; "));
        bounded(":150 many pipes", &vec!["cat"; 1000].join(" | "));
        bounded(
            ":165 repeated brace patterns",
            "echo {a,b}{c,d}{e,f}{g,h}{i,j}{k,l}{m,n}{o,p}",
        );
        bounded(
            ":179 many redirections",
            &format!("echo test {}", vec!["> /dev/null"; 500].join(" ")),
        );
        bounded(
            ":193 alternating quotes",
            &format!("echo {}", r#""a"'b'"#.repeat(10000)),
        );

        // :209 brace expansion during execution is bounded (does not hang).
        let brace = shell().exec("echo {a,b}{c,d}{e,f}{g,h}{i,j}{k,l}{m,n}{o,p}{q,r}{s,t}");
        assert_eq!(brace.exit_code, 0, ":209 exit");

        // :220 range expansion is bounded: completes with limited output.
        let range = shell().exec("echo {1..100000}");
        assert_eq!(range.exit_code, 0, ":220 exit");
        assert!(range.stdout.len() < 1_000_000, ":220 output length bound");
    }
}
