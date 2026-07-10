import click


@click.command(help="example plugin for python-m")
@click.option("--name", default="world", show_default=True, help="The object to greet.")
def hello(name: str,) -> None:
    click.echo(f"hello {name}")

if __name__ == "__main__":
    hello()
