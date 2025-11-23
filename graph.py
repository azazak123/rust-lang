import sys
import matplotlib.pyplot as plt

def read_points(filename):
    xs = []
    ys = []
    with open(filename, "r") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue

            parts = line.split()
            if len(parts) != 2:
                raise ValueError(f"Невірний формат рядка: '{line}' (потрібно: x y)")

            x, y = map(float, parts)
            xs.append(x)
            ys.append(y)

    return xs, ys


def plot_points(xs, ys):
    plt.figure(figsize=(8, 6))
    plt.scatter(xs, ys, s=2)   # точки не з’єднуються
    plt.xlabel("X")
    plt.ylabel("Y")
    plt.title("Графік точок із файлу")
    plt.grid(True)
    plt.show()


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Використання: python3 plot_points.py <filename>")
        sys.exit(1)

    filename = sys.argv[1]
    xs, ys = read_points(filename)
    plot_points(xs, ys)
