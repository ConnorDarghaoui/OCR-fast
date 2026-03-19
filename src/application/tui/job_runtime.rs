use crate::application::pipeline::{OcrPipeline, PipelineEvent};
use crate::domain::{LanguageConfig, ProcessingProfile};
use crate::infrastructure::postprocessors::TextPostprocessor;
use crate::interfaces::ports::{DocumentParserPort, OcrEnginePort};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Receiver;
use std::sync::Arc;

/// Lote de eventos emitidos por un job entre dos ticks del event loop.
pub(crate) struct JobRuntimeBatch {
    pub(crate) id_trabajo: String,
    pub(crate) eventos: Vec<PipelineEvent>,
}

/// Coordinador de jobs OCR en background y su telemetría visible.
///
/// Este componente encapsula canales, flags de cancelación y progreso por job.
/// `AppState` conserva la mutación del modelo de dominio, pero deja de conocer
/// cómo se materializa la ejecución concurrente de cada pipeline.
pub(crate) struct JobRuntimeState {
    receptores_activos: HashMap<String, Receiver<PipelineEvent>>,
    cancelaciones_activas: HashMap<String, Arc<AtomicBool>>,
    fases: HashMap<String, String>,
    progresos: HashMap<String, f32>,
}

impl JobRuntimeState {
    /// Construye el estado vacío de coordinación de jobs.
    pub(crate) fn new() -> Self {
        Self {
            receptores_activos: HashMap::new(),
            cancelaciones_activas: HashMap::new(),
            fases: HashMap::new(),
            progresos: HashMap::new(),
        }
    }

    /// Lanza un pipeline OCR en background y registra sus estructuras auxiliares.
    pub(crate) fn iniciar(
        &mut self,
        id_trabajo: String,
        ruta: PathBuf,
        perfil: ProcessingProfile,
        idioma: LanguageConfig,
        analizador: Arc<dyn DocumentParserPort>,
        motor: Arc<dyn OcrEnginePort>,
    ) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.receptores_activos.insert(id_trabajo.clone(), rx);

        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.cancelaciones_activas
            .insert(id_trabajo.clone(), Arc::clone(&cancel_flag));

        self.fases
            .insert(id_trabajo.clone(), "Iniciando...".to_string());
        self.progresos.insert(id_trabajo.clone(), 0.0);

        std::thread::spawn(move || {
            let postprocesador = TextPostprocessor::new().with_language(&idioma.primary);
            let pipeline = OcrPipeline::new(analizador, Arc::clone(&motor))
                .with_postprocessor(Arc::new(postprocesador));

            match pipeline.procesar_documento(&ruta, &perfil, Some(&tx), Some(&cancel_flag)) {
                Ok(documento) => {
                    let _ = tx.send(PipelineEvent::Completado(documento));
                }
                Err(error) => {
                    let _ = tx.send(PipelineEvent::Error(error));
                }
            }
        });
    }

    /// Solicita cancelación cooperativa del job indicado.
    pub(crate) fn solicitar_cancelacion(&self, id_trabajo: &str) -> bool {
        match self.cancelaciones_activas.get(id_trabajo) {
            Some(flag) => {
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    /// Drena todos los canales activos sin bloquear el event loop.
    pub(crate) fn recolectar_eventos(&self) -> Vec<JobRuntimeBatch> {
        self.receptores_activos
            .keys()
            .cloned()
            .filter_map(|id_trabajo| {
                let eventos = self.recolectar_eventos_de_job(&id_trabajo);
                if eventos.is_empty() {
                    None
                } else {
                    Some(JobRuntimeBatch {
                        id_trabajo,
                        eventos,
                    })
                }
            })
            .collect()
    }

    /// Actualiza la fase y el progreso agregado del job.
    pub(crate) fn actualizar_fase(&mut self, id_trabajo: &str, fase: String, progreso: f32) {
        self.fases.insert(id_trabajo.to_string(), fase);
        self.progresos.insert(id_trabajo.to_string(), progreso);
    }

    /// Enriquce la fase actual con el progreso de páginas visibles.
    pub(crate) fn actualizar_pagina_actual(
        &mut self,
        id_trabajo: &str,
        pagina_actual: u32,
        total_paginas: u32,
    ) {
        if let Some(fase_actual) = self.fases.get_mut(id_trabajo) {
            *fase_actual = format!(
                "{} (pagina {}/{})",
                fase_actual, pagina_actual, total_paginas
            );
        }
    }

    /// Elimina todo el estado auxiliar asociado a un job.
    pub(crate) fn limpiar_job(&mut self, id_trabajo: &str) {
        self.receptores_activos.remove(id_trabajo);
        self.cancelaciones_activas.remove(id_trabajo);
        self.fases.remove(id_trabajo);
        self.progresos.remove(id_trabajo);
    }

    /// Conserva únicamente el estado auxiliar de los jobs aún presentes.
    pub(crate) fn retener_activos(&mut self, ids_activos: &[String]) {
        self.fases.retain(|id, _| ids_activos.contains(id));
        self.progresos.retain(|id, _| ids_activos.contains(id));
        self.receptores_activos
            .retain(|id, _| ids_activos.contains(id));
        self.cancelaciones_activas
            .retain(|id, _| ids_activos.contains(id));
    }

    /// Retorna la fase visible del job si sigue rastreado.
    pub(crate) fn fase(&self, id_trabajo: &str) -> Option<&str> {
        self.fases.get(id_trabajo).map(String::as_str)
    }

    /// Retorna el progreso agregado del job si sigue rastreado.
    pub(crate) fn progreso(&self, id_trabajo: &str) -> Option<f32> {
        self.progresos.get(id_trabajo).copied()
    }

    /// Indica si todavía hay pipelines activos bajo seguimiento.
    pub(crate) fn hay_activos(&self) -> bool {
        !self.receptores_activos.is_empty()
    }

    fn recolectar_eventos_de_job(&self, id_trabajo: &str) -> Vec<PipelineEvent> {
        let mut eventos = Vec::new();

        if let Some(receiver) = self.receptores_activos.get(id_trabajo) {
            loop {
                match receiver.try_recv() {
                    Ok(evento) => eventos.push(evento),
                    Err(_) => break,
                }
            }
        }

        eventos
    }
}

#[cfg(test)]
mod tests {
    use super::JobRuntimeState;

    #[test]
    fn test_limpiar_job_purga_estado_auxiliar() {
        let mut runtime = JobRuntimeState::new();
        runtime.actualizar_fase("job-1", "OCR".to_string(), 0.4);
        runtime.limpiar_job("job-1");

        assert!(runtime.fase("job-1").is_none());
        assert!(runtime.progreso("job-1").is_none());
        assert!(!runtime.hay_activos());
    }
}
