from __future__ import annotations

import click

@click.command(help="example plugin for py-bin")
@click.option("-n", default="py-bin", show_default=True, help="The object to greet.")
def hello4(n: str,) -> None:
    click.echo(f"hello {n}")

if __name__ == "__main__":
    hello4()