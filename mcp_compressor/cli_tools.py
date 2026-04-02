"""CLI tool helpers: name mutation, arg parsing, help text formatting."""

from __future__ import annotations

import json
import re
from typing import Any

from fastmcp.tools import Tool

TOP_LEVEL_HELP_TEMPLATE = """\
{prefix}{cli_name} - {server_description}

When relevant, outputs from this CLI will prefer using the TOON format for more efficient representation of data.

USAGE:
  {cli_name} <subcommand> [options]

SUBCOMMANDS:
{subcommand_table}

Run '{cli_name} <subcommand> --help' for subcommand usage.\
"""

TOOL_HELP_TEMPLATE = """\
{cli_name} {subcommand} - {description}

USAGE:
  {cli_name} {subcommand} [options]

OPTIONS:
{options}
"""

SCHEMA_PREFIX = """\
Functionality associated with the {cli_name} toolset is provided via the `{cli_name}` CLI. Access the functionality \
below via the CLI rather than through structured tool/function calling.\
"""


def tool_name_to_subcommand(tool_name: str) -> str:
    """Convert a snake_case or camelCase MCP tool name to a kebab-case CLI subcommand.

    Examples:
        get_confluence_page -> get-confluence-page
        getConfluencePage   -> get-confluence-page
        createJiraIssue     -> create-jira-issue
        getjiraissue        -> getjiraissue  (already lowercase, no splits possible)
    """
    # Insert hyphens before uppercase letters (camelCase -> kebab-case)
    kebab = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1-\2", tool_name)
    kebab = re.sub(r"([a-z\d])([A-Z])", r"\1-\2", kebab)
    # Replace underscores with hyphens and lowercase everything
    return kebab.replace("_", "-").lower()


def subcommand_to_tool_name(subcommand: str) -> str:
    """Convert a kebab-case CLI subcommand back to a snake_case MCP tool name."""
    return subcommand.replace("-", "_")


def sanitize_cli_name(name: str) -> str:
    """Sanitize a name for use as a CLI command / script name.

    Rules:
    - lowercase
    - only a-z, 0-9, - and _ allowed; everything else becomes -
    - collapse consecutive separators
    - strip leading/trailing separators
    - if the result starts with a digit, prepend "mcp-"
    """
    lowered = name.lower()
    sanitized = re.sub(r"[^a-z0-9_-]", "-", lowered)
    sanitized = re.sub(r"[-_]{2,}", "-", sanitized)
    sanitized = sanitized.strip("-_")
    if not sanitized:
        sanitized = "mcp"
    if sanitized[0].isdigit():
        sanitized = "mcp-" + sanitized
    return sanitized


def format_top_level_help(
    cli_name: str, server_description: str, tools: list[Tool], for_tool_schema: bool = False
) -> str:
    """Format top-level --help output listing all subcommands."""
    prefix = ""
    if for_tool_schema:
        prefix = SCHEMA_PREFIX.format(cli_name=cli_name)
    subcommand_table = []
    for tool in sorted(tools, key=lambda t: t.name):
        subcommand = tool_name_to_subcommand(tool.name)
        desc = (tool.description or "").strip().splitlines()[0].split(".")[0] if tool.description else ""
        subcommand_table.append(f"  {subcommand:<35} {desc}")
    return TOP_LEVEL_HELP_TEMPLATE.format(
        prefix=prefix,
        cli_name=cli_name,
        server_description=server_description,
        subcommand_table="\n".join(subcommand_table),
    )


def format_tool_help(cli_name: str, tool: Tool) -> str:
    """Format per-tool --help output."""
    subcommand = tool_name_to_subcommand(tool.name)
    description = (tool.description or "").strip()

    properties: dict[str, Any] = tool.parameters.get("properties", {})
    required: list[str] = tool.parameters.get("required", [])

    options_list: list[str] = []

    for prop_name, prop_schema in properties.items():
        flag = "--" + prop_name.replace("_", "-")
        prop_type = _schema_type_label(prop_schema)
        req_label = "(required)" if prop_name in required else "(optional)"
        prop_desc = prop_schema.get("description", "")
        options_list.append(f"  {flag} {prop_type:<10} {req_label} {prop_desc}".rstrip())

        # For JSON types, inline the schema immediately below the option line
        unwrapped = _unwrap_nullable(prop_schema)
        raw_type = unwrapped.get("type")
        if raw_type == "object" or (raw_type is None and unwrapped.get("properties")):
            # Strip fields that are noisy / not useful for CLI users
            schema_display = {k: v for k, v in prop_schema.items() if k not in ("description", "title")}
            if schema_display:
                schema_json = json.dumps(schema_display, separators=(",", ":"))
                options_list.append(f"    Values must be a JSON string with the following schema: {schema_json}")

    # --quiet is a universal flag available on every subcommand (not in the tool schema)
    options_list.append("  --quiet                    (optional) Truncate large output to a short preview")

    return TOOL_HELP_TEMPLATE.format(
        cli_name=cli_name,
        subcommand=subcommand,
        description=description or "(no description)",
        options="\n".join(options_list),
    )


def _unwrap_nullable(schema: dict[str, Any]) -> dict[str, Any]:
    """If schema is anyOf/oneOf with one null and one other type, return the non-null schema.

    This handles Pydantic's rendering of ``str | None`` as
    ``{"anyOf": [{"type": "string"}, {"type": "null"}]}``.
    Returns the original schema unchanged if the pattern doesn't match.
    """
    for key in ("anyOf", "oneOf"):
        variants = schema.get(key)
        if isinstance(variants, list):
            non_null = [v for v in variants if v.get("type") != "null"]
            if len(non_null) == 1:
                return non_null[0]
    return schema


def _schema_type_label(schema: dict[str, Any]) -> str:
    """Return a short type label for a JSON Schema property."""
    schema = _unwrap_nullable(schema)
    t = schema.get("type")
    if t is None:
        return "JSON"
    if t == "object":
        return "JSON"
    if t == "array":
        items = schema.get("items", {})
        item_type = items.get("type", "any") if isinstance(items, dict) else "any"
        item_label = "JSON" if item_type == "object" else item_type.upper()
        return f"[{item_label}]"
    return t.upper() if isinstance(t, str) else "JSON"


def build_help_tool_description(
    cli_name: str, server_description: str, tools: list[Tool], on_path: bool = False
) -> str:
    """Build the description string for the single <server_name>_help MCP tool.

    Reuses ``format_top_level_help`` for the subcommand table so the MCP tool
    description and the CLI ``--help`` output stay in sync.
    """
    invoke = cli_name if on_path else f"./{cli_name}"
    help_text = format_top_level_help(cli_name, server_description, tools, for_tool_schema=True)
    return (
        f"{help_text}\n\n"
        f"Run '{invoke} --help' in the shell for usage.\n"
        f"Run '{invoke} <subcommand> --help' for per-command help.\n"
        f"Run '{invoke} <subcommand> [options]' to invoke a tool."
    )


def parse_argv_to_tool_input(argv: list[str], tool: Tool) -> dict[str, Any]:
    """Parse CLI argv into a tool_input dict based on the tool's JSON Schema.

    Supports:
    - --flag value          -> string/number/integer
    - --flag val --flag val -> array (repeated)
    - --flag                -> boolean (True)
    - --json '{"k":"v"}'   -> raw JSON object override
    - --no-flag             -> boolean False

    Args:
        argv: List of CLI arguments (without the subcommand itself).
        tool: The MCP tool whose schema drives parsing.

    Returns:
        Dict suitable for use as tool_input.

    Raises:
        ValueError: If required args are missing or args are unrecognised.
    """
    if len(argv) == 1 and argv[0] == "--json":
        raise ValueError('--json requires a value: --json \'{"key":"value"}\'')

    # Raw JSON escape hatch
    if len(argv) >= 2 and argv[0] == "--json":
        return json.loads(argv[1])

    properties: dict[str, Any] = tool.parameters.get("properties", {})
    required: list[str] = tool.parameters.get("required", [])

    # Build a lookup: kebab-case flag -> snake_case prop name
    flag_to_prop: dict[str, str] = {}
    for prop_name in properties:
        flag_to_prop[prop_name.replace("_", "-")] = prop_name
        flag_to_prop[prop_name] = prop_name  # allow snake_case too

    result: dict[str, Any] = {}
    i = 0
    while i < len(argv):
        i = _parse_single_arg(argv, i, flag_to_prop, properties, result)

    # Check required
    missing = [r for r in required if r not in result]
    if missing:
        missing_flags = ", ".join(f"--{m.replace('_', '-')}" for m in missing)
        raise ValueError(f"Missing required option(s): {missing_flags}")

    return result


def _parse_single_arg(
    argv: list[str],
    i: int,
    flag_to_prop: dict[str, str],
    properties: dict[str, Any],
    result: dict[str, Any],
) -> int:
    """Parse a single CLI argument at position *i* and return the new index."""
    arg = argv[i]
    if not arg.startswith("--"):
        raise ValueError(f"Unexpected positional argument: {arg!r}. Use --flag value syntax.")

    flag = arg[2:]

    # Boolean --no-flag
    if flag.startswith("no-"):
        prop_name = flag_to_prop.get(flag[3:]) or flag_to_prop.get(flag[3:].replace("-", "_"))
        if prop_name and properties[prop_name].get("type") == "boolean":
            result[prop_name] = False
            return i + 1

    prop_name = flag_to_prop.get(flag)
    if prop_name is None:
        raise ValueError(f"Unknown option: --{flag}")

    prop_schema = _unwrap_nullable(properties[prop_name])
    # Use sentinel None so we can distinguish "no type key" from "type: string"
    prop_type = prop_schema.get("type")

    if prop_type == "boolean":
        return _parse_boolean_arg(argv, i, prop_name, result)

    if i + 1 >= len(argv):
        raise ValueError(f"--{flag} requires a value.")

    value_str = argv[i + 1]
    _store_typed_value(value_str, prop_name, prop_schema, prop_type or "", result)
    return i + 2


def _parse_boolean_arg(argv: list[str], i: int, prop_name: str, result: dict[str, Any]) -> int:
    """Handle a boolean flag, optionally consuming a true/false value."""
    if i + 1 < len(argv) and argv[i + 1].lower() in ("true", "false"):
        result[prop_name] = argv[i + 1].lower() == "true"
        return i + 2
    result[prop_name] = True
    return i + 1


def _store_typed_value(
    value_str: str, prop_name: str, prop_schema: dict[str, Any], prop_type: str, result: dict[str, Any]
) -> None:
    """Coerce *value_str* according to *prop_type* and store it in *result*."""
    if prop_type == "array":
        item_type = (
            prop_schema.get("items", {}).get("type", "string")
            if isinstance(prop_schema.get("items"), dict)
            else "string"
        )
        parsed_value = _coerce_value(value_str, item_type)
        if prop_name in result:
            result[prop_name].append(parsed_value)
        else:
            result[prop_name] = [parsed_value]
    elif prop_type in ("integer", "number"):
        result[prop_name] = _coerce_value(value_str, prop_type)
    elif prop_type == "string":
        result[prop_name] = value_str
    else:
        # object, unknown/complex types (oneOf, anyOf, $ref, no 'type' key, etc.):
        # attempt JSON parsing so the user can pass structured values as JSON strings.
        # Fall back to raw string if JSON parsing fails.
        result[prop_name] = _try_parse_json(value_str)


def _try_parse_json(value_str: str) -> Any:
    """Attempt to parse *value_str* as JSON; return the raw string on failure."""
    try:
        return json.loads(value_str)
    except (json.JSONDecodeError, ValueError):
        return value_str


def _coerce_value(value_str: str, type_name: str) -> Any:
    """Coerce a string value to the appropriate Python type."""
    if type_name == "integer":
        try:
            return int(value_str)
        except ValueError:
            raise ValueError(f"Expected integer, got {value_str!r}") from None
    if type_name == "number":
        try:
            return float(value_str)
        except ValueError:
            raise ValueError(f"Expected number, got {value_str!r}") from None
    if type_name == "boolean":
        if value_str.lower() in ("true", "1", "yes"):
            return True
        if value_str.lower() in ("false", "0", "no"):
            return False
        raise ValueError(f"Expected boolean, got {value_str!r}")
    if type_name == "object" or type_name not in ("string", "boolean", "integer", "number", "array"):
        # object and complex/unknown types: attempt JSON parsing
        return _try_parse_json(value_str)
    return value_str
