//! Um retângulo em pixels da foto — a região que um gesto sujou.

/// `[x, x + largura) × [y, y + altura)`, em pixels da foto inteira de pé.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Retangulo {
    pub x: u32,
    pub y: u32,
    pub largura: u32,
    pub altura: u32,
}

impl Retangulo {
    pub fn novo(x: u32, y: u32, largura: u32, altura: u32) -> Self {
        Self {
            x,
            y,
            largura,
            altura,
        }
    }

    /// A foto inteira.
    pub fn inteiro(largura: u32, altura: u32) -> Self {
        Self::novo(0, 0, largura, altura)
    }

    pub fn vazio(&self) -> bool {
        self.largura == 0 || self.altura == 0
    }

    pub fn direita(&self) -> u32 {
        self.x + self.largura
    }

    pub fn baixo(&self) -> u32 {
        self.y + self.altura
    }

    /// O menor retângulo que cobre os dois. O vazio não conta.
    pub fn uniao(&self, outro: &Retangulo) -> Retangulo {
        if self.vazio() {
            return *outro;
        }
        if outro.vazio() {
            return *self;
        }
        let x = self.x.min(outro.x);
        let y = self.y.min(outro.y);
        Retangulo::novo(
            x,
            y,
            self.direita().max(outro.direita()) - x,
            self.baixo().max(outro.baixo()) - y,
        )
    }

    /// A parte dentro de `largura × altura`.
    pub fn limitado(&self, largura: u32, altura: u32) -> Retangulo {
        let x = self.x.min(largura);
        let y = self.y.min(altura);
        Retangulo::novo(
            x,
            y,
            self.direita().min(largura).saturating_sub(x),
            self.baixo().min(altura).saturating_sub(y),
        )
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn a_uniao_ignora_o_vazio_e_cobre_os_dois() {
        let a = Retangulo::novo(10, 10, 5, 5);
        assert_eq!(Retangulo::default().uniao(&a), a);
        assert_eq!(
            a.uniao(&Retangulo::novo(0, 12, 2, 10)),
            Retangulo::novo(0, 10, 15, 12)
        );
        assert_eq!(
            Retangulo::novo(90, 90, 20, 20).limitado(100, 95),
            Retangulo::novo(90, 90, 10, 5)
        );
    }
}
