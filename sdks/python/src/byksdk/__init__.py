__version__ = "0.1.0"


from .network import (
    ensure_port_available,
    get_private_networks,
    PortBusyError,
)
from .cli import (
    check_port,
    colored_key_value,
    copy_to_clipboard,
    echo_network_urls,
    open_browser,
    wait_for_server_ready,
)
from .web import (
    R,
    create_spa,
    get_client_ip,
)
from .logging import get_logger
from .plugin import PluginContext, plugin

__all__ = [
    # network
    "ensure_port_available",
    "get_private_networks",
    "PortBusyError",
    # cli
    "check_port",
    "colored_key_value",
    "copy_to_clipboard",
    "echo_network_urls",
    "open_browser",
    "wait_for_server_ready",
    # web
    "R",
    "create_spa",
    "get_client_ip",
    # logging
    "get_logger",
    # plugin
    "PluginContext",
    "plugin",
]