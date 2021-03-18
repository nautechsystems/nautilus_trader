import pathlib


def update_stubs():
    import fsspec
    from fsspec.implementations.github import GithubFileSystem

    def _valid_file(f):
        return pathlib.Path(f).name.startswith("streaming_")

    ghfs = GithubFileSystem(org="liampauling", repo="betfair")

    for fn in filter(_valid_file, ghfs.ls("tests/resources/")):
        with ghfs.open(fn) as remote:
            with fsspec.open(f"./{fn.split('/')[-1]}", "wb") as local:
                print(f"Wrote update for {fn}")
                local.write(remote.read())


if __name__ == "__main__":
    pass
    # Uncomment to run

    # update_stubs()
