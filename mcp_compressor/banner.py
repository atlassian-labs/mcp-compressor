import shutil

from .types import CompressionLevel

TITLE = """\
\033[32m█▀▄▀█ █▀▀ █▀█   █▀▀ █▀█ █▀▄▀█ █▀█ █▀█ █▀▀ █▀▀ █▀▀ █▀█ █▀█\033[0m
\033[32m█ ▀ █ █▄▄ █▀▀   █▄▄ █▄█ █ ▀ █ █▀▀ █▀▄ ██▄ ▄▄█ ▄▄█ █▄█ █▀▄\033[0m\
"""


def print_banner(
    server_name: str | None, transport_type: str, stats: dict, compression_level: CompressionLevel
) -> None:
    """Print the startup banner with server information and compression stats.

    Args:
        server_name: The name of the backend server, if provided.
        transport_type: The transport type being used (stdio, http, sse).
        stats: Compression statistics from get_compression_stats().
        compression_level: The compression level being used.
    """
    # Get terminal width
    columns = min(shutil.get_terminal_size().columns, 80)
    if columns < 63:
        # Terminal too narrow to display banner properly
        return

    header = "╭" + "─" * (columns - 2) + "╮"
    footer = "╰" + "─" * (columns - 2) + "╯"
    separator = "├" + "─" * (columns - 2) + "┤"
    blank_line = "│" + " " * (columns - 2) + "│"

    banner = [header, blank_line]
    for line in TITLE.splitlines():
        banner.append(_pad_line(line, columns + 9, center=True))
    if server_name:
        banner.append(blank_line)
        banner.append(_pad_line(f"\033[32m●\033[0m Backend server name: {server_name}", columns + 9))
    banner.append(blank_line)
    banner.append(_pad_line(f"\033[32m●\033[0m Backend server transport: {transport_type.upper()}", columns + 9))
    banner.append(blank_line)
    banner.append(_pad_line("\033[32m●\033[0m Docs: https://atlassian-labs.github.io/mcp-compressor/", columns + 9))
    banner.append(blank_line)
    banner.append(separator)
    banner.append(blank_line)
    banner.append(_pad_line(f"📊 Compression Statistics (current = {compression_level.capitalize()}):", columns - 1))
    banner.append(blank_line)
    for line in _format_compression_chart(stats, columns, compression_level):
        banner.append(line)
    banner.append(blank_line)
    banner.append(footer)

    print("\n".join(banner))


def _format_compression_chart(stats: dict, width: int, compression_level: CompressionLevel) -> list[str]:
    """Format compression statistics as a visual bar chart.

    Args:
        stats: Dictionary containing compression statistics from get_compression_stats().
        width: Total width of the chart area.
        compression_level: The compression level being used.

    Returns:
        Formatted strings with bar chart visualization.
    """
    width -= 25
    original_size = stats["original_schema_size"]
    compressed_sizes = stats["compressed_schema_sizes"]

    lines = []

    # Original size bar (100%)
    bar = "█" * width
    lines.append(_pad_line(f"Original {bar} 100.0%", width + 25))

    # Compressed size bars for each level
    for level in [CompressionLevel.LOW, CompressionLevel.MEDIUM, CompressionLevel.HIGH, CompressionLevel.MAX]:
        size = compressed_sizes[level]
        ratio = size / original_size if original_size > 0 else 0
        filled = int(ratio * width)
        bar = "█" * filled + "░" * (width - filled)
        pct = ratio * 100
        label = f"{level.value.capitalize():<8}"
        line = _pad_line(f"{label} {bar} {pct:5.1f}%", width + 25)
        if level == compression_level:
            # Use blue color for the current compression level
            blue_end = line.find("░")
            if blue_end == -1:
                blue_end = len(line) - 2
            line = line[:2] + "\033[1;32m" + line[2:blue_end] + "\033[0m" + line[blue_end:]
        lines.append(line)

    return lines


def _pad_line(line: str, total_width: int, center: bool = False) -> str:
    if center:
        padding_total = total_width - 6 - len(line)
        padding_left = padding_total // 2
        line = " " * padding_left + line
    return "│  " + f"{line:<{total_width - 6}}" + "  │"
