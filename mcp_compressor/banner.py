import re
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

    # Content width is the available space inside the box (columns - 6 for borders and padding)
    content_width = columns - 6

    header = "╭" + "─" * (columns - 2) + "╮"
    footer = "╰" + "─" * (columns - 2) + "╯"
    separator = "├" + "─" * (columns - 2) + "┤"
    blank_line = "│" + " " * (columns - 2) + "│"

    banner = [header, blank_line]
    for line in TITLE.splitlines():
        banner.append(_pad_line(line, content_width, center=True))
    if server_name:
        banner.append(blank_line)
        banner.append(_pad_line(f"\033[32m●\033[0m Backend server name: {server_name}", content_width))
    banner.append(blank_line)
    banner.append(_pad_line(f"\033[32m●\033[0m Backend server transport: {transport_type.upper()}", content_width))
    banner.append(blank_line)
    banner.append(_pad_line("\033[32m●\033[0m Docs: https://atlassian-labs.github.io/mcp-compressor/", content_width))
    banner.append(blank_line)
    banner.append(separator)
    banner.append(blank_line)
    banner.append(_pad_line(f"📊 Compression Statistics (current = {compression_level.capitalize()}):", content_width))
    banner.append(blank_line)
    for line in _format_compression_chart(stats, content_width, compression_level):
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
    # Reserve space for label, percentage, and spacing: "Medium   " (9) + " " (1) + "100.0%" (6) = 16 chars
    chart_width = width - 16
    original_size = stats["original_schema_size"]
    compressed_sizes = stats["compressed_schema_sizes"]

    lines = []

    # Original size bar (100%)
    bar = "█" * chart_width
    lines.append(_pad_line(f"Original {bar} 100.0%", width))

    # Compressed size bars for each level
    for level in [CompressionLevel.LOW, CompressionLevel.MEDIUM, CompressionLevel.HIGH, CompressionLevel.MAX]:
        size = compressed_sizes[level]
        ratio = size / original_size if original_size > 0 else 0
        # Clamp filled to not exceed chart_width
        filled = min(int(ratio * chart_width), chart_width)
        bar = "█" * filled + "░" * (chart_width - filled)
        pct = ratio * 100
        label = f"{level.value.capitalize():<8}"
        line = _pad_line(f"{label} {bar} {pct:5.1f}%", width)
        if level == compression_level:
            # Use green color for the current compression level
            blue_end = line.find("░")
            if blue_end == -1:
                blue_end = len(line) - 2
            line = line[:2] + "\033[1;32m" + line[2:blue_end] + "\033[0m" + line[blue_end:]
        lines.append(line)

    return lines


def _pad_line(line: str, total_width: int, center: bool = False) -> str:
    """Pad a line to fit within a box of the given width.

    Args:
        line: The line content (may include ANSI codes).
        total_width: Total width available for content (excluding box borders).
        center: Whether to center the line.

    Returns:
        A padded line with box borders.
    """
    # Calculate actual content width (excluding ANSI codes)
    clean_line = re.sub(r"\033\[[0-9;]*m", "", line)
    clean_width = len(clean_line)

    if center:
        padding_total = total_width - clean_width
        padding_left = padding_total // 2
        padding_right = padding_total - padding_left
        return "│  " + " " * padding_left + line + " " * padding_right + "  │"
    else:
        padding_right = total_width - clean_width
        return "│  " + line + " " * padding_right + "  │"
