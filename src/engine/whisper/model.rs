//! Modèle Whisper (poids Hugging Face) pour CrisperWhisper, d'après candle-transformers
//! (MIT/Apache-2.0), avec deux ajouts : cache clé/valeur de l'auto-attention du décodeur
//! (un jeton à la fois) et capture de l'attention croisée des têtes d'alignement,
//! nécessaire aux horodatages mot à mot.

use candle_core::{DType, Device, IndexOp, Module, Result, Tensor, D};
use candle_nn::{embedding, linear, linear_no_bias, Conv1d, Conv1dConfig, Embedding, LayerNorm, Linear, VarBuilder};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub num_mel_bins: usize,
    pub max_source_positions: usize,
    pub d_model: usize,
    pub encoder_attention_heads: usize,
    pub encoder_layers: usize,
    pub vocab_size: usize,
    pub max_target_positions: usize,
    pub decoder_attention_heads: usize,
    pub decoder_layers: usize,
    #[serde(default)]
    pub encoder_ffn_dim: usize,
    #[serde(default)]
    pub decoder_ffn_dim: usize,
}

fn layer_norm(size: usize, vb: VarBuilder) -> Result<LayerNorm> {
    Ok(LayerNorm::new(vb.get(size, "weight")?, vb.get(size, "bias")?, 1e-5))
}

fn conv1d(cin: usize, cout: usize, stride: usize, vb: VarBuilder) -> Result<Conv1d> {
    let cfg = Conv1dConfig { padding: 1, stride, groups: 1, dilation: 1, cudnn_fwd_algo: None };
    Ok(Conv1d::new(vb.get((cout, cin, 3), "weight")?, Some(vb.get(cout, "bias")?), cfg))
}

#[derive(Debug, Clone)]
struct Attention {
    q: Linear,
    k: Linear,
    v: Linear,
    out: Linear,
    n_head: usize,
    /// Clés/valeurs déjà calculées : jetons précédents (auto-attention) ou encodeur (attention croisée).
    cache: Option<(Tensor, Tensor)>,
}

impl Attention {
    fn load(n_state: usize, n_head: usize, vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            q: linear(n_state, n_state, vb.pp("q_proj"))?,
            k: linear_no_bias(n_state, n_state, vb.pp("k_proj"))?,
            v: linear(n_state, n_state, vb.pp("v_proj"))?,
            out: linear(n_state, n_state, vb.pp("out_proj"))?,
            n_head,
            cache: None,
        })
    }

    fn heads(&self, x: &Tensor) -> Result<Tensor> {
        let (b, t, s) = x.dims3()?;
        x.reshape((b, t, self.n_head, s / self.n_head))?.transpose(1, 2)?.contiguous()
    }

    /// Renvoie la sortie et, si demandé, les poids d'attention [têtes, jetons, positions].
    fn forward(&mut self, x: &Tensor, kv: KvMode, mask: Option<&Tensor>, keep_weights: bool) -> Result<(Tensor, Option<Tensor>)> {
        let q = self.q.forward(x)?;
        let (k, v) = match kv {
            KvMode::Encoder => (self.heads(&self.k.forward(x)?)?, self.heads(&self.v.forward(x)?)?),
            KvMode::Cross(xa) => {
                if self.cache.is_none() {
                    self.cache = Some((self.heads(&self.k.forward(xa)?)?, self.heads(&self.v.forward(xa)?)?));
                }
                self.cache.clone().unwrap()
            }
            KvMode::SelfCached => {
                let (k, v) = (self.heads(&self.k.forward(x)?)?, self.heads(&self.v.forward(x)?)?);
                let (k, v) = match &self.cache {
                    Some((pk, pv)) => (Tensor::cat(&[pk, &k], 2)?, Tensor::cat(&[pv, &v], 2)?),
                    None => (k, v),
                };
                self.cache = Some((k.clone(), v.clone()));
                (k, v)
            }
        };
        let (_, n_ctx, n_state) = q.dims3()?;
        let scale = ((n_state / self.n_head) as f64).powf(-0.25);
        let q = (self.heads(&q)? * scale)?;
        let k = (k.transpose(2, 3)? * scale)?;
        let mut qk = q.matmul(&k)?.to_dtype(DType::F32)?;
        if let Some(mask) = mask {
            let total = qk.dim(D::Minus1)?;
            let mask = mask.i((total - n_ctx..total, 0..total))?;
            qk = qk.broadcast_add(&mask)?;
        }
        let w = candle_nn::ops::softmax_last_dim(&qk)?;
        let wv = w.to_dtype(v.dtype())?.matmul(&v)?.transpose(1, 2)?.flatten_from(2)?;
        let weights = if keep_weights { Some(w.squeeze(0)?) } else { None };
        Ok((self.out.forward(&wv)?, weights))
    }
}

enum KvMode<'a> {
    Encoder,
    Cross(&'a Tensor),
    SelfCached,
}

#[derive(Debug, Clone)]
struct Block {
    attn: Attention,
    attn_ln: LayerNorm,
    cross: Option<(Attention, LayerNorm)>,
    fc1: Linear,
    fc2: Linear,
    mlp_ln: LayerNorm,
}

impl Block {
    fn load(n_state: usize, n_head: usize, ffn: usize, cross: bool, vb: VarBuilder) -> Result<Self> {
        let cross = if cross {
            Some((Attention::load(n_state, n_head, vb.pp("encoder_attn"))?, layer_norm(n_state, vb.pp("encoder_attn_layer_norm"))?))
        } else {
            None
        };
        Ok(Self {
            attn: Attention::load(n_state, n_head, vb.pp("self_attn"))?,
            attn_ln: layer_norm(n_state, vb.pp("self_attn_layer_norm"))?,
            cross,
            fc1: linear(n_state, ffn, vb.pp("fc1"))?,
            fc2: linear(ffn, n_state, vb.pp("fc2"))?,
            mlp_ln: layer_norm(n_state, vb.pp("final_layer_norm"))?,
        })
    }

    fn forward(&mut self, x: &Tensor, xa: Option<&Tensor>, mask: Option<&Tensor>, keep_cross: bool) -> Result<(Tensor, Option<Tensor>)> {
        let mode = if xa.is_some() { KvMode::SelfCached } else { KvMode::Encoder };
        let (a, _) = self.attn.forward(&self.attn_ln.forward(x)?, mode, mask, false)?;
        let mut x = (x + a)?;
        let mut weights = None;
        if let (Some((attn, ln)), Some(xa)) = (&mut self.cross, xa) {
            let (a, w) = attn.forward(&ln.forward(&x)?, KvMode::Cross(xa), None, keep_cross)?;
            x = (&x + a)?;
            weights = w;
        }
        let mlp = self.fc2.forward(&self.fc1.forward(&self.mlp_ln.forward(&x)?)?.gelu()?)?;
        Ok(((x + mlp)?, weights))
    }

    fn reset(&mut self) {
        self.attn.cache = None;
        if let Some((a, _)) = &mut self.cross {
            a.cache = None;
        }
    }
}

pub struct Encoder {
    conv1: Conv1d,
    conv2: Conv1d,
    pos: Tensor,
    blocks: Vec<Block>,
    ln: LayerNorm,
}

impl Encoder {
    fn load(vb: VarBuilder, c: &Config) -> Result<Self> {
        let ffn = if c.encoder_ffn_dim > 0 { c.encoder_ffn_dim } else { 4 * c.d_model };
        Ok(Self {
            conv1: conv1d(c.num_mel_bins, c.d_model, 1, vb.pp("conv1"))?,
            conv2: conv1d(c.d_model, c.d_model, 2, vb.pp("conv2"))?,
            pos: vb.get((c.max_source_positions, c.d_model), "embed_positions.weight")?,
            blocks: (0..c.encoder_layers)
                .map(|i| Block::load(c.d_model, c.encoder_attention_heads, ffn, false, vb.pp(format!("layers.{i}"))))
                .collect::<Result<_>>()?,
            ln: layer_norm(c.d_model, vb.pp("layer_norm"))?,
        })
    }

    /// mel [1, n_mels, 3000] → états [1, 1500, d_model].
    pub fn forward(&mut self, mel: &Tensor) -> Result<Tensor> {
        let x = self.conv1.forward(mel)?.gelu()?;
        let x = self.conv2.forward(&x)?.gelu()?.transpose(1, 2)?;
        let n = x.dim(1)?;
        let mut x = x.broadcast_add(&self.pos.narrow(0, 0, n)?)?;
        for b in self.blocks.iter_mut() {
            x = b.forward(&x, None, None, false)?.0;
        }
        self.ln.forward(&x)
    }
}

pub struct Decoder {
    embed: Embedding,
    pos: Tensor,
    blocks: Vec<Block>,
    ln: LayerNorm,
    mask: Tensor,
    /// Nombre de jetons déjà passés (position du prochain).
    pub offset: usize,
}

impl Decoder {
    fn load(vb: VarBuilder, c: &Config) -> Result<Self> {
        let ffn = if c.decoder_ffn_dim > 0 { c.decoder_ffn_dim } else { 4 * c.d_model };
        let n = c.max_target_positions;
        let mask: Vec<f32> = (0..n).flat_map(|i| (0..n).map(move |j| if j > i { f32::NEG_INFINITY } else { 0.0 })).collect();
        Ok(Self {
            embed: embedding(c.vocab_size, c.d_model, vb.pp("embed_tokens"))?,
            pos: vb.get((n, c.d_model), "embed_positions.weight")?,
            blocks: (0..c.decoder_layers)
                .map(|i| Block::load(c.d_model, c.decoder_attention_heads, ffn, true, vb.pp(format!("layers.{i}"))))
                .collect::<Result<_>>()?,
            ln: layer_norm(c.d_model, vb.pp("layer_norm"))?,
            mask: Tensor::from_vec(mask, (n, n), vb.device())?,
            offset: 0,
        })
    }

    pub fn reset(&mut self) {
        self.offset = 0;
        self.blocks.iter_mut().for_each(Block::reset);
    }

    /// Passe de nouveaux jetons (après ceux déjà vus). Renvoie les logits du dernier jeton [vocab]
    /// et, pour chaque tête d'alignement demandée (couche, tête), l'attention croisée des nouveaux
    /// jetons : [nouveaux jetons, positions de l'encodeur].
    pub fn step(&mut self, tokens: &[u32], xa: &Tensor, heads: &[(usize, usize)]) -> Result<(Tensor, Vec<Tensor>)> {
        let dev = xa.device();
        let n = tokens.len();
        let x = self.embed.forward(&Tensor::new(tokens, dev)?.unsqueeze(0)?)?;
        let mut x = x.broadcast_add(&self.pos.narrow(0, self.offset, n)?)?;
        let mut captured = Vec::with_capacity(heads.len());
        let mask = (n > 1 || self.offset > 0).then_some(&self.mask);
        for (li, b) in self.blocks.iter_mut().enumerate() {
            let want = heads.iter().any(|&(l, _)| l == li);
            let (y, w) = b.forward(&x, Some(xa), mask, want)?;
            x = y;
            if let Some(w) = w {
                for &(l, h) in heads.iter().filter(|(l, _)| *l == li) {
                    let _ = l;
                    captured.push(w.i(h)?);
                }
            }
        }
        self.offset += n;
        let x = self.ln.forward(&x.i((.., n - 1..n, ..))?)?;
        let logits = x.squeeze(0)?.matmul(&self.embed.embeddings().t()?)?.squeeze(0)?.to_dtype(DType::F32)?;
        // ordre des têtes capturées = ordre des couches ; on le remet dans l'ordre demandé
        let mut by_layer: Vec<(usize, usize)> = heads.to_vec();
        by_layer.sort_by_key(|&(l, _)| l);
        let mut out = vec![captured[0].clone(); heads.len()];
        let mut k = 0;
        for &(l, h) in &by_layer {
            let pos = heads.iter().position(|&p| p == (l, h)).unwrap();
            out[pos] = captured[k].clone();
            k += 1;
        }
        Ok((logits, out))
    }
}

pub struct Whisper {
    pub encoder: Encoder,
    pub decoder: Decoder,
    pub config: Config,
    pub device: Device,
    pub dtype: DType,
}

impl Whisper {
    pub fn load(weights: &std::path::Path, config: Config, device: &Device, dtype: DType) -> Result<Self> {
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[weights], dtype, device)? };
        Ok(Self {
            encoder: Encoder::load(vb.pp("model.encoder"), &config)?,
            decoder: Decoder::load(vb.pp("model.decoder"), &config)?,
            config,
            device: device.clone(),
            dtype,
        })
    }
}
