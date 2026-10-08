// Works in the browser and in Node when run with F5.
const message = "Bienvenue dans Forge IDE !";
console.log(message);

if (typeof document !== "undefined") {
  document.querySelector("#hello")?.addEventListener("click", () => {
    document.querySelector("#result").textContent = message;
  });
}
