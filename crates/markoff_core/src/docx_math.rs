pub(crate) struct MathNode {
    name: String,
    value: Option<String>,
    text: String,
    children: Vec<Self>,
}

impl MathNode {
    pub(crate) fn new(name: String, value: Option<String>) -> Self {
        Self {
            name,
            value,
            text: String::new(),
            children: Vec::new(),
        }
    }

    pub(crate) fn push_child(&mut self, child: Self) {
        self.children.push(child);
    }

    pub(crate) fn push_text(&mut self, text: &str) {
        if self.name == "t" {
            self.text.push_str(text);
        }
    }

    pub(crate) fn is_display(&self) -> bool {
        self.name == "oMathPara"
    }

    pub(crate) fn into_markdown(self, display: bool) -> String {
        let latex = self.render();
        if display {
            format!("$${latex}$$")
        } else {
            format!("${latex}$")
        }
    }

    fn render(&self) -> String {
        match self.name.as_str() {
            "t" => self.render_text(),
            "sSup" => format!("{}^{{{}}}", self.child_text("e"), self.child_text("sup")),
            "sSub" => format!("{}_{{{}}}", self.child_text("e"), self.child_text("sub")),
            "sSubSup" => format!(
                "{}_{{{}}}^{{{}}}",
                self.child_text("e"),
                self.child_text("sub"),
                self.child_text("sup")
            ),
            "sPre" => format!(
                "{}_{{{}}}^{{{}}}{}",
                "",
                self.child_text("sub"),
                self.child_text("sup"),
                self.child_text("e")
            ),
            "limLow" => format!("{}_{{{}}}", self.child_text("e"), self.child_text("lim")),
            "limUpp" => format!("{}^{{{}}}", self.child_text("e"), self.child_text("lim")),
            "f" => format!(
                "\\frac{{{}}}{{{}}}",
                self.child_text("num"),
                self.child_text("den")
            ),
            "rad" => {
                let degree = self.child_text("deg");
                let expression = self.child_text("e");
                if degree.is_empty() {
                    format!("\\sqrt{{{expression}}}")
                } else {
                    format!("\\sqrt[{degree}]{{{expression}}}")
                }
            }
            "m" => self.render_matrix(),
            "d" => {
                let begin = self.property_value("begChr").unwrap_or("(");
                let end = self.property_value("endChr").unwrap_or(")");
                format!("\\left{begin}{}\\right{end}", self.child_text("e"))
            }
            "nary" => {
                let operator = match self.property_value("chr").unwrap_or("∫") {
                    "∫" => "\\int",
                    "∑" => "\\sum",
                    "∏" => "\\prod",
                    other => other,
                };
                let sub = self.child_text("sub");
                let sup = self.child_text("sup");
                let limits = match (sub.is_empty(), sup.is_empty()) {
                    (false, false) => format!("_{{{sub}}}^{{{sup}}}"),
                    (false, true) => format!("_{{{sub}}}"),
                    (true, false) => format!("^{{{sup}}}"),
                    (true, true) => String::new(),
                };
                format!("{operator}{limits}{}", self.child_text("e"))
            }
            "acc" => format!("\\hat{{{}}}", self.child_text("e")),
            "bar" => format!("\\overline{{{}}}", self.child_text("e")),
            _ if self.name.ends_with("Pr") => String::new(),
            _ => self.render_children(),
        }
    }

    fn render_matrix(&self) -> String {
        let rows = self
            .children
            .iter()
            .filter(|child| child.name == "mr")
            .map(|row| {
                row.children
                    .iter()
                    .filter(|cell| cell.name == "e")
                    .map(Self::render)
                    .collect::<Vec<_>>()
                    .join(" & ")
            })
            .collect::<Vec<_>>();
        format!("\\begin{{matrix}}{}\\end{{matrix}}", rows.join(" \\\\ "))
    }

    fn render_text(&self) -> String {
        [
            ("±", "\\pm"),
            ("≤", "\\le"),
            ("≥", "\\ge"),
            ("≠", "\\neq"),
            ("≈", "\\approx"),
            ("∞", "\\infty"),
            ("∑", "\\sum"),
            ("∏", "\\prod"),
            ("∫", "\\int"),
            ("×", "\\times"),
            ("÷", "\\div"),
            ("⇒", "\\Rightarrow"),
            ("⇔", "\\Leftrightarrow"),
            ("→", "\\rightarrow"),
            ("←", "\\leftarrow"),
            ("↔", "\\leftrightarrow"),
        ]
        .into_iter()
        .fold(self.text.clone(), |text, (symbol, latex)| {
            text.replace(symbol, latex)
        })
    }

    fn child_text(&self, name: &str) -> String {
        self.children
            .iter()
            .find(|child| child.name == name)
            .map(Self::render)
            .unwrap_or_default()
    }

    fn render_children(&self) -> String {
        self.children.iter().map(Self::render).collect()
    }

    fn property_value(&self, name: &str) -> Option<&str> {
        self.children
            .iter()
            .find(|child| child.name.ends_with("Pr"))
            .and_then(|properties| properties.find_value(name))
    }

    fn find_value(&self, name: &str) -> Option<&str> {
        if self.name == name {
            return self.value.as_deref();
        }
        self.children
            .iter()
            .find_map(|child| child.find_value(name))
    }
}
