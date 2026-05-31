#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedName {
    pub short_name: String,
    pub module_specifier: String,
    pub function_name: String,
}

fn parse_name(tag: &str, name: &str) -> Option<ParsedName> {
    let mut parts = name.split("//");
    let prefix = parts.next()?;
    let module_specifier = parts.next()?;
    let function_name_parts = parts.collect::<Vec<_>>();

    if prefix != tag || module_specifier.is_empty() || function_name_parts.is_empty() {
        return None;
    }

    let function_name = function_name_parts.join("//");
    let mut short_name = function_name.rsplit('/').next().unwrap_or("").to_owned();
    let module_short_name = module_short_name(module_specifier);

    if matches!(short_name.as_str(), "default" | "__default") && !module_short_name.is_empty() {
        short_name = module_short_name;
    }

    Some(ParsedName {
        short_name,
        module_specifier: module_specifier.to_owned(),
        function_name,
    })
}

fn module_short_name(module_specifier: &str) -> String {
    let without_version = if module_specifier.starts_with("./") {
        module_specifier
    } else {
        match module_specifier.rfind('@') {
            Some(0) => "",
            Some(index) => &module_specifier[..index],
            None => module_specifier.split('@').next().unwrap_or(""),
        }
    };

    without_version.rsplit('/').next().unwrap_or("").to_owned()
}

pub fn parse_workflow_name(name: &str) -> Option<ParsedName> {
    parse_name("workflow", name)
}

pub fn parse_step_name(name: &str) -> Option<ParsedName> {
    parse_name("step", name)
}

pub fn parse_class_name(name: &str) -> Option<ParsedName> {
    parse_name("class", name)
}

pub fn format_step_name(name: &str) -> String {
    format_parsed_name(parse_step_name(name), name)
}

pub fn format_workflow_name(name: &str) -> String {
    format_parsed_name(parse_workflow_name(name), name)
}

fn format_parsed_name(parsed: Option<ParsedName>, fallback: &str) -> String {
    parsed.map_or_else(
        || fallback.to_owned(),
        |parsed| format!("{} ({})", parsed.short_name, parsed.module_specifier),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(short_name: &str, module_specifier: &str, function_name: &str) -> ParsedName {
        ParsedName {
            short_name: short_name.to_owned(),
            module_specifier: module_specifier.to_owned(),
            function_name: function_name.to_owned(),
        }
    }

    #[test]
    fn upstream_parse_name_cases() {
        assert_eq!(
            parse_workflow_name("workflow//./src/workflows/order//handleOrder"),
            Some(parsed(
                "handleOrder",
                "./src/workflows/order",
                "handleOrder"
            ))
        );
        assert_eq!(
            parse_workflow_name("workflow//mypackage@1.0.0//handleOrder"),
            Some(parsed("handleOrder", "mypackage@1.0.0", "handleOrder"))
        );
        assert_eq!(
            parse_workflow_name("workflow//@myorg/tasks@2.0.0//processOrder"),
            Some(parsed("processOrder", "@myorg/tasks@2.0.0", "processOrder"))
        );
        assert_eq!(
            parse_workflow_name("workflow//./src/app//nested//function//name"),
            Some(parsed("name", "./src/app", "nested//function//name"))
        );
        assert_eq!(parse_workflow_name("invalid"), None);
        assert_eq!(parse_workflow_name("workflow//"), None);
        assert_eq!(parse_workflow_name("step//path//fn"), None);
        assert_eq!(
            parse_workflow_name("workflow//./path//"),
            Some(parsed("", "./path", ""))
        );
        assert_eq!(
            parse_workflow_name("workflow//./src/jobs/order//default"),
            Some(parsed("order", "./src/jobs/order", "default"))
        );
        assert_eq!(
            parse_workflow_name("workflow//mypackage@1.0.0//default"),
            Some(parsed("mypackage", "mypackage@1.0.0", "default"))
        );
        assert_eq!(
            parse_workflow_name("workflow//@myorg/tasks@2.0.0//default"),
            Some(parsed("tasks", "@myorg/tasks@2.0.0", "default"))
        );

        assert_eq!(
            parse_step_name("step//./src/workflows/order//processOrder"),
            Some(parsed(
                "processOrder",
                "./src/workflows/order",
                "processOrder"
            ))
        );
        assert_eq!(
            parse_step_name("step//mypackage@1.0.0//processOrder"),
            Some(parsed("processOrder", "mypackage@1.0.0", "processOrder"))
        );
        assert_eq!(
            parse_step_name("step//./app/api/generate/route//handleStep"),
            Some(parsed(
                "handleStep",
                "./app/api/generate/route",
                "handleStep"
            ))
        );
        assert_eq!(parse_step_name("invalid"), None);
        assert_eq!(parse_step_name("step//"), None);
        assert_eq!(parse_step_name("workflow//path//fn"), None);
        assert_eq!(
            parse_step_name("step//./path//"),
            Some(parsed("", "./path", ""))
        );
        assert_eq!(
            parse_step_name("step//builtin//__builtin_fetch"),
            Some(parsed("__builtin_fetch", "builtin", "__builtin_fetch"))
        );
        assert_eq!(
            parse_step_name("step//./src/jobs/order//processOrder/innerStep"),
            Some(parsed(
                "innerStep",
                "./src/jobs/order",
                "processOrder/innerStep"
            ))
        );
        assert_eq!(
            parse_step_name("step//./src/jobs/order//MyClass.staticMethod"),
            Some(parsed(
                "MyClass.staticMethod",
                "./src/jobs/order",
                "MyClass.staticMethod"
            ))
        );
        assert_eq!(
            parse_step_name("step//./src/jobs/order//MyClass#instanceMethod"),
            Some(parsed(
                "MyClass#instanceMethod",
                "./src/jobs/order",
                "MyClass#instanceMethod"
            ))
        );

        assert_eq!(
            parse_class_name("class//./src/models/point//Point"),
            Some(parsed("Point", "./src/models/point", "Point"))
        );
        assert_eq!(
            parse_class_name("class//point@0.0.1//Point"),
            Some(parsed("Point", "point@0.0.1", "Point"))
        );
        assert_eq!(
            parse_class_name("class//@myorg/models@1.2.3//UserData"),
            Some(parsed("UserData", "@myorg/models@1.2.3", "UserData"))
        );
        assert_eq!(
            parse_class_name("class//./workflows/user-signup//UserData"),
            Some(parsed("UserData", "./workflows/user-signup", "UserData"))
        );
        assert_eq!(parse_class_name("invalid"), None);
        assert_eq!(parse_class_name("class//"), None);
        assert_eq!(parse_class_name("step//path//fn"), None);
        assert_eq!(parse_class_name("workflow//path//fn"), None);

        assert_eq!(
            format_step_name("step//./workflows/1_simple//add"),
            "add (./workflows/1_simple)"
        );
        assert_eq!(
            format_workflow_name("workflow//./workflows/1_simple//simple"),
            "simple (./workflows/1_simple)"
        );
        assert_eq!(
            format_step_name("step//@myorg/tasks@2.0.0//processOrder"),
            "processOrder (@myorg/tasks@2.0.0)"
        );
        assert_eq!(
            format_step_name("step//./workflows/order//processOrder/innerStep"),
            "innerStep (./workflows/order)"
        );
        assert_eq!(format_step_name("something-weird"), "something-weird");
        assert_eq!(
            format_workflow_name("step//wrong-tag//fn"),
            "step//wrong-tag//fn"
        );
    }
}
