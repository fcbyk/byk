from __future__ import annotations

import click

@click.command(help="second command")
def world():
    click.echo("second command -> hello world")

if __name__ == "__main__":
    world()
