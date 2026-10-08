from pathlib import Path


def greet(name: str) -> str:
    return f"Bienvenue dans {name} !"


if __name__ == "__main__":
    print(greet("Forge IDE"))
    print(f"Projet : {Path.cwd()}")
