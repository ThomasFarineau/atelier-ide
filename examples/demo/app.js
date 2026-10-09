// Works in the browser and in Node when run with F5.
const message = "Bienvenue dans Atelier IDE !";
console.log(message);

if (typeof document !== "undefined") {
  document.querySelector("#hello")?.addEventListener("click", () => {
    document.querySelector("#result").textContent = message;
  });
}

export function testname() {
    
}
