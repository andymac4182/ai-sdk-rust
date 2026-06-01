use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseItem {
    pub patterns: Vec<Word>,
    pub body: Vec<Statement>,
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
    Semicolon,
    Newline,
    DoubleSemicolon,
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

pub fn parse(input: &str) -> ShellResult<Script> {
    Parser::new(input)?.parse()
}

pub fn serialize(script: &Script) -> String {
    serialize_statements(&script.statements)
}

pub fn collect_command_names(script: &Script) -> Vec<String> {
    let mut names = std::collections::BTreeSet::new();
    collect_statements(&script.statements, &mut names);
    names.into_iter().collect()
}

struct Lexer {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    column: usize,
}

impl Lexer {
    fn new(input: &str) -> Self {
        Self {
            chars: input.chars().collect(),
            pos: 0,
            line: 1,
            column: 1,
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
                tokens.push(Token { kind, line, column });
                continue;
            }

            let word = self.read_word(line, column)?;
            tokens.push(Token {
                kind: TokenKind::Word(word),
                line,
                column,
            });
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
    let mut output = serialize_pipeline(&statement.pipelines[0]);
    for (operator, pipeline) in statement
        .operators
        .iter()
        .zip(statement.pipelines.iter().skip(1))
    {
        output.push_str(match operator {
            ListOperator::And => " && ",
            ListOperator::Or => " || ",
        });
        output.push_str(&serialize_pipeline(pipeline));
    }
    if statement.background {
        if !output.is_empty() {
            output.push(' ');
        }
        output.push('&');
    }
    output
}

fn serialize_pipeline(pipeline: &Pipeline) -> String {
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
        output.push_str(&serialize_command(command));
    }
    output
}

fn serialize_command(command: &Command) -> String {
    match command {
        Command::Simple(command) => serialize_simple_command(command),
        Command::If(command) => serialize_if(command),
        Command::For(command) => serialize_for(command),
        Command::While(command) => serialize_loop("while", command),
        Command::Until(command) => serialize_loop("until", command),
        Command::Case(command) => serialize_case(command),
        Command::FunctionDef(function) => format!(
            "{}() {}{}",
            function.name,
            serialize_command(&function.body),
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

fn serialize_simple_command(command: &SimpleCommand) -> String {
    let mut parts = Vec::new();
    parts.extend(command.assignments.iter().map(serialize_assignment));
    if let Some(name) = &command.name {
        parts.push(serialize_word(name));
    }
    parts.extend(command.args.iter().map(serialize_word));
    parts.extend(command.redirections.iter().map(serialize_redirection));
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
        output.push_str(" ;;");
    }
    output.push_str(" esac");
    output.push_str(&serialize_redirections(&command.redirections));
    output
}

fn serialize_redirections(redirections: &[Redirection]) -> String {
    redirections
        .iter()
        .map(|redirection| format!(" {}", serialize_redirection(redirection)))
        .collect::<String>()
}

fn serialize_redirection(redirection: &Redirection) -> String {
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
    output.push(' ');
    output.push_str(&serialize_word(&redirection.target));
    output
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
            TokenKind::DoubleSemicolon => stop_case_end,
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
            if self.is_command_terminator() {
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
        let variable = self
            .take_word_text()
            .ok_or_else(|| self.error_here("expected for variable"))?;
        if !is_valid_name(&variable) {
            return Err(self.error_previous("invalid for variable"));
        }

        self.skip_separators();
        let mut words = Vec::new();
        if self.current_word_is("in") {
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
            if matches!(self.current().kind, TokenKind::DoubleSemicolon) {
                self.advance();
            }
            items.push(CaseItem { patterns, body });
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

    fn is_command_terminator(&self) -> bool {
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
        ) || self.current_word_is_any(&["then", "else", "elif", "fi", "do", "done", "esac"])
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionLimits {
    pub max_loop_iterations: usize,
    pub max_commands: usize,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_loop_iterations: 10_000,
            max_commands: 100_000,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ShellState {
    env: BTreeMap<String, String>,
    arrays: BTreeMap<String, Vec<String>>,
    aliases: BTreeMap<String, String>,
    functions: BTreeMap<String, FunctionDef>,
    positionals: Vec<String>,
    local_scopes: Vec<BTreeMap<String, Option<String>>>,
    last_status: i32,
    pipefail: bool,
    exited: Option<i32>,
    command_count: usize,
    alias_depth: usize,
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
        let old_functions = std::mem::take(&mut self.state.functions);
        self.state.exited = None;
        self.state.command_count = 0;
        let output = match parse(source) {
            Ok(script) => self.exec_script(&script),
            Err(error) => ExecOutput {
                stdout: String::new(),
                stderr: format!("{error}\n"),
                exit_code: 2,
            },
        };
        self.state.functions = old_functions;
        self.state.exited = None;
        output
    }

    pub fn exec_script(&mut self, script: &Script) -> ExecOutput {
        self.execute_statements(&script.statements)
    }

    fn execute_statements(&mut self, statements: &[Statement]) -> ExecOutput {
        let mut output = ExecOutput::default();
        for statement in statements {
            if self.state.exited.is_some() {
                break;
            }
            output.append(self.execute_statement(statement));
        }
        if let Some(code) = self.state.exited {
            output.exit_code = code;
        }
        output
    }

    fn execute_statement(&mut self, statement: &Statement) -> ExecOutput {
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
        self.state.last_status = output.exit_code;
        output
    }

    fn execute_pipeline(&mut self, pipeline: &Pipeline) -> ExecOutput {
        let mut aggregate = ExecOutput::default();
        let mut stdin = String::new();
        let mut statuses = Vec::new();

        for (index, command) in pipeline.commands.iter().enumerate() {
            let command_stdin = std::mem::take(&mut stdin);
            let result = self.execute_command(command, command_stdin);
            statuses.push(result.exit_code);
            if index + 1 == pipeline.commands.len() {
                aggregate.stdout.push_str(&result.stdout);
            } else {
                stdin = result.stdout;
            }
            aggregate.stderr.push_str(&result.stderr);
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
        if let Err(error) = self.count_command() {
            return ExecOutput {
                stdout: String::new(),
                stderr: format!("{error}\n"),
                exit_code: 125,
            };
        }

        match command {
            Command::Simple(command) => self.execute_simple_command(command, stdin),
            Command::If(command) => self.execute_if(command),
            Command::For(command) => self.execute_for(command),
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
                let output = self.execute_statements(body);
                self.state.env = saved_env;
                self.state.arrays = saved_arrays;
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
        let Some(name_word) = &command.name else {
            for assignment in assignments {
                self.apply_assignment(assignment);
            }
            return ExecOutput::default();
        };

        let Some(name) = self.expand_word(name_word, true).into_iter().next() else {
            return ExecOutput::default();
        };
        let args = command
            .args
            .iter()
            .flat_map(|word| self.expand_word(word, true))
            .collect::<Vec<_>>();
        let redirections = command
            .redirections
            .iter()
            .map(|redirection| AppliedRedirection {
                fd: redirection.fd,
                operator: redirection.operator,
                target: self.expand_word_to_string(&redirection.target),
            })
            .collect::<Vec<_>>();

        if let Some(alias) = self.state.aliases.get(&name).cloned()
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
            return output;
        }

        if let Some(function) = self.state.functions.get(&name).cloned() {
            return self.call_function(function, args);
        }

        if let Some(output) = self.execute_builtin(&name, &args) {
            return self.apply_redirections(output, &redirections);
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
        self.apply_redirections(
            ExecOutput {
                stdout: result.stdout,
                stderr: result.stderr,
                exit_code: result.exit_code,
            },
            &redirections,
        )
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
            "set" => {
                if args == ["-o", "pipefail"] {
                    self.state.pipefail = true;
                } else if args == ["+o", "pipefail"] {
                    self.state.pipefail = false;
                }
                Some(ExecOutput::default())
            }
            "alias" => {
                for arg in args {
                    if let Some((name, value)) = split_assignment_text(arg) {
                        self.state.aliases.insert(name, value);
                    }
                }
                Some(ExecOutput::default())
            }
            "exit" => {
                let code = args
                    .first()
                    .and_then(|arg| arg.parse::<i32>().ok())
                    .unwrap_or(0);
                self.state.exited = Some(code);
                Some(ExecOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: code,
                })
            }
            _ => None,
        }
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

    fn call_function(&mut self, function: FunctionDef, args: Vec<String>) -> ExecOutput {
        let saved_positionals = std::mem::replace(&mut self.state.positionals, args);
        self.state.local_scopes.push(BTreeMap::new());
        let output = self.execute_command(&function.body, String::new());
        self.state.local_scopes.pop();
        self.state.positionals = saved_positionals;
        output
    }

    fn execute_if(&mut self, command: &IfCommand) -> ExecOutput {
        let mut output = ExecOutput::default();
        for clause in &command.clauses {
            let condition = self.execute_statements(&clause.condition);
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
        let mut iterations = 0usize;
        let words = command
            .words
            .iter()
            .flat_map(|word| self.expand_word(word, true))
            .collect::<Vec<_>>();
        for value in words {
            iterations += 1;
            if iterations > self.limits.max_loop_iterations {
                output.stderr.push_str("too many iterations\n");
                output.exit_code = 125;
                break;
            }
            self.state.assign_var(command.variable.clone(), value);
            output.append(self.execute_statements(&command.body));
        }
        output
    }

    fn execute_loop(&mut self, command: &LoopCommand, is_while: bool) -> ExecOutput {
        let mut output = ExecOutput::default();
        for _ in 0..self.limits.max_loop_iterations {
            let condition = self.execute_statements(&command.condition);
            let condition_status = condition.exit_code;
            output.append(condition);
            let should_run = if is_while {
                condition_status == 0
            } else {
                condition_status != 0
            };
            if !should_run {
                output.exit_code = 0;
                return output;
            }
            output.append(self.execute_statements(&command.body));
            if self.state.exited.is_some() {
                return output;
            }
        }
        output.stderr.push_str("too many iterations\n");
        output.exit_code = 125;
        output
    }

    fn execute_case(&mut self, command: &CaseCommand) -> ExecOutput {
        let value = self.expand_word_to_string(&command.word);
        for item in &command.items {
            for pattern in &item.patterns {
                let pattern = self.expand_word_to_string(pattern);
                if pattern_matches(&pattern, &value) {
                    return self.execute_statements(&item.body);
                }
            }
        }
        ExecOutput::default()
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
        &self,
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
                | RedirectionOperator::ReadWrite => 0,
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

    fn count_command(&mut self) -> Result<(), &'static str> {
        self.state.command_count += 1;
        if self.state.command_count > self.limits.max_commands {
            Err("too many commands")
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
            values
                .into_iter()
                .flat_map(|value| {
                    value
                        .split_whitespace()
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
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

    fn eval_arithmetic(&self, source: &str) -> i64 {
        ArithmeticEvaluator::new(source, &self.state).parse()
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
    End,
}

struct ArithmeticEvaluator<'a> {
    tokens: Vec<ArithToken>,
    pos: usize,
    state: &'a ShellState,
}

impl<'a> ArithmeticEvaluator<'a> {
    fn new(source: &str, state: &'a ShellState) -> Self {
        Self {
            tokens: tokenize_arithmetic(source),
            pos: 0,
            state,
        }
    }

    fn parse(&mut self) -> i64 {
        self.parse_expr(1)
    }

    fn parse_expr(&mut self, min_prec: u8) -> i64 {
        let mut left = self.parse_prefix();
        while let ArithToken::Op(operator) = self.current().clone() {
            let Some((precedence, right_assoc)) = arith_precedence(&operator) else {
                break;
            };
            if precedence < min_prec {
                break;
            }
            self.advance();
            let next_min = if right_assoc {
                precedence
            } else {
                precedence + 1
            };
            let right = self.parse_expr(next_min);
            left = eval_arith_binary(&operator, left, right);
        }
        left
    }

    fn parse_prefix(&mut self) -> i64 {
        match self.advance() {
            ArithToken::Number(value) => value,
            ArithToken::Ident(name) => self
                .state
                .lookup_var(&name)
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(0),
            ArithToken::Op(operator) if matches!(operator.as_str(), "+" | "-" | "!" | "~") => {
                let value = self.parse_prefix();
                match operator.as_str() {
                    "+" => value,
                    "-" => -value,
                    "!" => i64::from(value == 0),
                    "~" => !value,
                    _ => value,
                }
            }
            ArithToken::LeftParen => {
                let value = self.parse_expr(1);
                if matches!(self.current(), ArithToken::RightParen) {
                    self.advance();
                }
                value
            }
            _ => 0,
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
            while chars
                .get(pos)
                .is_some_and(|character| character.is_ascii_digit())
            {
                pos += 1;
            }
            let text = chars[start..pos].iter().collect::<String>();
            tokens.push(ArithToken::Number(text.parse::<i64>().unwrap_or(0)));
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
        let mut matched = false;
        for operator in [
            "**", "<<", ">>", "<=", ">=", "==", "!=", "&&", "||", "+", "-", "*", "/", "%", "<",
            ">", "&", "|", "^", "!", "~",
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
                "printf" => CommandResult::success(invocation.args.join("")),
                "cat" => fake_cat(&invocation, files),
                "grep" => fake_grep(&invocation, files),
                "wc" if invocation.args.first().map(String::as_str) == Some("-l") => {
                    let count = invocation.stdin.lines().count();
                    CommandResult::success(format!("{count}\n"))
                }
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

    fn shell() -> Interpreter<FakeCommands> {
        Interpreter::new(FakeCommands::default())
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
}
