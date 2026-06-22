from __future__ import annotations

import click

@click.command(help="example plugin for py-module two")
def world():
    click.echo("second command -> hello world")

if __name__ == "__main__":
    world()
