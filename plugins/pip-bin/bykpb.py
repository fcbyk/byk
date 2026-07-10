import click


@click.command(help="example plugin for pip-bin")
@click.option("-n", default="pip-bin", show_default=True, help="The object to greet.")
def bykpb(n: str) -> None:
    click.echo(f"hello {n}")


if __name__ == "__main__":
    bykpb()
