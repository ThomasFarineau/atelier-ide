interface Project {
  name: string;
  languages: string[];
}

const project: Project = {
  name: "Atelier IDE",
  languages: ["HTML", "CSS", "JavaScript", "TypeScript", "Java", "Python", "Rust"],
};

console.log(`${project.name} : ${project.languages.join(", ")}`);
