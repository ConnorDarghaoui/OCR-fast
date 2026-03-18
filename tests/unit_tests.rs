#[cfg(test)]
mod domain_tests {
    use ocrfast::domain::{
        Block, BlockType, Dimensions, Document, Job, JobStatus, Page, ProcessingProfile, Rectangle,
    };

    #[test]
    fn test_document_creation() {
        let document = Document {
            id: "test-id".to_string(),
            source_path: std::path::PathBuf::from("/tmp/test.pdf"),
            pages: vec![],
            metadata: std::collections::HashMap::new(),
        };

        assert_eq!(document.id, "test-id");
        assert_eq!(
            document.source_path,
            std::path::PathBuf::from("/tmp/test.pdf")
        );
    }

    #[test]
    fn test_page_creation() {
        let page = Page {
            number: 1,
            dimensions: Dimensions {
                width: 100,
                height: 200,
            },
            blocks: vec![],
            image_data: None,
        };

        assert_eq!(page.number, 1);
        assert_eq!(page.dimensions.width, 100);
        assert_eq!(page.dimensions.height, 200);
    }

    #[test]
    fn test_block_creation() {
        let block = Block {
            block_type: BlockType::Text,
            bounding_box: Rectangle {
                x: 10,
                y: 20,
                width: 100,
                height: 50,
            },
            content: "Hola mundo".to_string(),
            confidence: 0.95,
            embedded_image: None,
            table_structure: None,
            reading_order: 0,
        };

        assert_eq!(block.block_type, BlockType::Text);
        assert_eq!(block.content, "Hola mundo");
        assert_eq!(block.confidence, 0.95);
    }

    #[test]
    fn test_job_creation() {
        let document = Document {
            id: "test-id".to_string(),
            source_path: std::path::PathBuf::from("/tmp/test.pdf"),
            pages: vec![],
            metadata: std::collections::HashMap::new(),
        };

        let job = Job {
            id: "job-test-id".to_string(),
            document,
            status: JobStatus::Queued,
            created_at: std::time::SystemTime::now(),
            completed_at: None,
            profile: ProcessingProfile::Balanced,
            error_message: None,
            formato_salida: Default::default(),
        };

        assert_eq!(job.id, "job-test-id");
        assert_eq!(job.status, JobStatus::Queued);
        assert_eq!(job.profile, ProcessingProfile::Balanced);
        assert!(job.error_message.is_none());
    }

    #[test]
    fn test_job_with_error() {
        let document = Document {
            id: "test-id".to_string(),
            source_path: std::path::PathBuf::from("/tmp/test.pdf"),
            pages: vec![],
            metadata: std::collections::HashMap::new(),
        };

        let job = Job {
            id: "job-failed".to_string(),
            document,
            status: JobStatus::Failed,
            created_at: std::time::SystemTime::now(),
            completed_at: Some(std::time::SystemTime::now()),
            profile: ProcessingProfile::Fast,
            error_message: Some("Error de prueba".to_string()),
            formato_salida: Default::default(),
        };

        assert_eq!(job.status, JobStatus::Failed);
        assert_eq!(job.error_message, Some("Error de prueba".to_string()));
    }

    #[test]
    fn test_processing_profile_default() {
        let profile = ProcessingProfile::default();
        assert_eq!(profile, ProcessingProfile::Balanced);
    }
}

#[cfg(test)]
mod infrastructure_tests {
    use ocrfast::domain::{
        Block, BlockType, Dimensions, Document, Job, JobStatus, Page, ProcessingProfile, Rectangle,
    };
    use ocrfast::infrastructure::job_store::{
        normalizar_jobs_al_arranque, FileJobStore, InMemoryJobStore, JobStore,
    };

    #[test]
    fn test_in_memory_job_store() {
        let store = InMemoryJobStore::new();

        let document = Document {
            id: "test-doc".to_string(),
            source_path: std::path::PathBuf::from("/tmp/test.pdf"),
            pages: vec![Page {
                number: 1,
                dimensions: Dimensions {
                    width: 100,
                    height: 200,
                },
                blocks: vec![Block {
                    block_type: BlockType::Text,
                    bounding_box: Rectangle {
                        x: 0,
                        y: 0,
                        width: 100,
                        height: 50,
                    },
                    content: "Test".to_string(),
                    confidence: 1.0,
                    embedded_image: None,
                    table_structure: None,
                    reading_order: 0,
                }],
                image_data: None,
            }],
            metadata: std::collections::HashMap::new(),
        };

        let job = Job {
            id: "test-job".to_string(),
            document,
            status: JobStatus::Queued,
            created_at: std::time::SystemTime::now(),
            completed_at: None,
            profile: ProcessingProfile::Balanced,
            error_message: None,
            formato_salida: Default::default(),
        };

        assert!(store.save(&job).is_ok());

        let retrieved = store.get("test-job");
        assert!(retrieved.is_ok());
        assert_eq!(retrieved.unwrap().id, "test-job");
    }

    #[test]
    fn test_job_store_update() {
        let store = InMemoryJobStore::new();

        let document = Document {
            id: "doc-update".to_string(),
            source_path: std::path::PathBuf::from("/tmp/update.pdf"),
            pages: vec![],
            metadata: std::collections::HashMap::new(),
        };

        let mut job = Job {
            id: "job-update".to_string(),
            document,
            status: JobStatus::Queued,
            created_at: std::time::SystemTime::now(),
            completed_at: None,
            profile: ProcessingProfile::Accurate,
            error_message: None,
            formato_salida: Default::default(),
        };

        store.save(&job).unwrap();

        job.status = JobStatus::Completed;
        job.completed_at = Some(std::time::SystemTime::now());
        store.update(&job).unwrap();

        let retrieved = store.get("job-update").unwrap();
        assert_eq!(retrieved.status, JobStatus::Completed);
        assert!(retrieved.completed_at.is_some());
    }

    fn job_de_prueba(id: &str) -> Job {
        Job {
            id: id.to_string(),
            document: Document {
                id: format!("doc-{}", id),
                source_path: std::path::PathBuf::from("/tmp/test.pdf"),
                pages: vec![],
                metadata: std::collections::HashMap::new(),
            },
            status: JobStatus::Completed,
            created_at: std::time::SystemTime::now(),
            completed_at: Some(std::time::SystemTime::now()),
            profile: ProcessingProfile::Balanced,
            error_message: None,
            formato_salida: Default::default(),
        }
    }

    #[test]
    fn test_file_job_store_ciclo_completo() {
        let directorio = tempfile::tempdir().expect("No se pudo crear directorio temporal");
        let ruta = directorio.path().join("jobs.json");
        let store = FileJobStore::with_path(&ruta);

        assert_eq!(store.list().unwrap().len(), 0);

        let job = job_de_prueba("file-job-1");
        store.save(&job).unwrap();
        assert_eq!(store.list().unwrap().len(), 1);

        let recuperado = store.get("file-job-1").unwrap();
        assert_eq!(recuperado.id, "file-job-1");
        assert_eq!(recuperado.status, JobStatus::Completed);

        let mut job_modificado = job.clone();
        job_modificado.status = JobStatus::Failed;
        job_modificado.error_message = Some("Error de prueba".to_string());
        store.update(&job_modificado).unwrap();

        let actualizado = store.get("file-job-1").unwrap();
        assert_eq!(actualizado.status, JobStatus::Failed);

        store.delete("file-job-1").unwrap();
        assert_eq!(store.list().unwrap().len(), 0);
        assert!(store.get("file-job-1").is_err());
    }

    #[test]
    fn test_file_job_store_persistencia_entre_instancias() {
        let directorio = tempfile::tempdir().expect("No se pudo crear directorio temporal");
        let ruta = directorio.path().join("jobs.json");

        {
            let store = FileJobStore::with_path(&ruta);
            store.save(&job_de_prueba("persistencia-1")).unwrap();
            store.save(&job_de_prueba("persistencia-2")).unwrap();
        }

        {
            let store = FileJobStore::with_path(&ruta);
            let jobs = store.list().unwrap();
            assert_eq!(jobs.len(), 2);
            let ids: Vec<&str> = jobs.iter().map(|j| j.id.as_str()).collect();
            assert!(ids.contains(&"persistencia-1"));
            assert!(ids.contains(&"persistencia-2"));
        }
    }

    #[test]
    fn test_normalizar_jobs_al_arranque_marca_processing_como_failed() {
        let mut jobs = vec![
            {
                let mut j = job_de_prueba("completado");
                j.status = JobStatus::Completed;
                j
            },
            {
                let mut j = job_de_prueba("en-progreso");
                j.status = JobStatus::Processing;
                j
            },
            {
                let mut j = job_de_prueba("fallido");
                j.status = JobStatus::Failed;
                j
            },
        ];

        normalizar_jobs_al_arranque(&mut jobs);

        let completado = jobs.iter().find(|j| j.id == "completado").unwrap();
        assert_eq!(completado.status, JobStatus::Completed);

        let interrumpido = jobs.iter().find(|j| j.id == "en-progreso").unwrap();
        assert_eq!(interrumpido.status, JobStatus::Failed);
        assert!(interrumpido.error_message.is_some());

        let fallido = jobs.iter().find(|j| j.id == "fallido").unwrap();
        assert_eq!(fallido.status, JobStatus::Failed);
    }
}

#[cfg(test)]
mod postprocessor_tests {
    use ocrfast::domain::{Block, BlockType, Dimensions, Document, Page, Rectangle};
    use ocrfast::infrastructure::postprocessors::TextPostprocessor;
    use ocrfast::interfaces::ports::PostprocessorPort;
    use std::collections::HashMap;

    fn documento_con_bloque(contenido: &str) -> Document {
        Document {
            id: "pp-test".to_string(),
            source_path: std::path::PathBuf::from("/tmp/pp.png"),
            pages: vec![Page {
                number: 1,
                dimensions: Dimensions {
                    width: 100,
                    height: 100,
                },
                image_data: None,
                blocks: vec![Block {
                    block_type: BlockType::Text,
                    bounding_box: Rectangle {
                        x: 0,
                        y: 0,
                        width: 100,
                        height: 20,
                    },
                    content: contenido.to_string(),
                    confidence: 0.9,
                    embedded_image: None,
                    table_structure: None,
                    reading_order: 0,
                }],
            }],
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn test_postprocessor_normaliza_unicode() {
        let pp = TextPostprocessor::with_config(true, false, true);
        let mut doc = documento_con_bloque("\u{FB01}le \u{FB02}oor");
        pp.postprocess(&mut doc).unwrap();
        assert_eq!(doc.pages[0].blocks[0].content, "file floor");
    }

    #[test]
    fn test_postprocessor_corrige_espacios() {
        let pp = TextPostprocessor::with_config(false, true, false);
        let mut doc = documento_con_bloque("  hola   mundo   ");
        pp.postprocess(&mut doc).unwrap();
        assert_eq!(doc.pages[0].blocks[0].content, "hola mundo");
    }

    #[test]
    fn test_postprocessor_comillas_tipograficas() {
        let pp = TextPostprocessor::with_config(false, false, true);
        let mut doc = documento_con_bloque("\u{201C}hola\u{201D} y \u{2018}mundo\u{2019}");
        pp.postprocess(&mut doc).unwrap();
        assert_eq!(doc.pages[0].blocks[0].content, "\"hola\" y 'mundo'");
    }

    /// Regresion critica: verificar que "barn" NO se convierte en "bam"
    /// tras eliminar la sustitucion antipatron "rn"→"m".
    #[test]
    fn test_postprocessor_no_aplica_rn_a_m() {
        let pp = TextPostprocessor::with_config(false, false, true);
        let palabras = ["barn", "internal", "arrange", "morning", "return", "corner"];
        for palabra in palabras {
            let mut doc = documento_con_bloque(palabra);
            pp.postprocess(&mut doc).unwrap();
            assert_eq!(
                doc.pages[0].blocks[0].content, palabra,
                "La palabra '{}' no debe ser modificada (antipatron rn->m eliminado)",
                palabra
            );
        }
    }

    #[test]
    fn test_postprocessor_corrige_palabras_ocr() {
        let pp = TextPostprocessor::with_config(false, false, true).with_language("eng");
        let mut doc = documento_con_bloque("tbe quick brown fox");
        pp.postprocess(&mut doc).unwrap();
        assert_eq!(doc.pages[0].blocks[0].content, "the quick brown fox");
    }
}

#[cfg(test)]
mod exporter_tests {
    use ocrfast::domain::{
        Block, BlockType, Dimensions, Document, Job, JobStatus, Page, ProcessingProfile, Rectangle,
        TableCell, TableStructure,
    };
    use ocrfast::infrastructure::exporters::{
        DocxExporter, JsonExporter, LatexExporter, TxtExporter,
    };
    use ocrfast::interfaces::ports::ExporterPort;
    use std::collections::HashMap;

    fn png_color_sintetico(ancho: u32, alto: u32, color: [u8; 3]) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(ancho, alto, image::Rgb(color));
        let mut buffer = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buffer, image::ImageFormat::Png)
            .unwrap();
        buffer.into_inner()
    }

    fn job_con_tabla(con_estructura: bool) -> Job {
        let tabla = if con_estructura {
            Some(TableStructure {
                num_rows: 2,
                num_cols: 2,
                rows: vec![
                    vec![
                        TableCell {
                            content: "Nombre".to_string(),
                            row_span: 1,
                            col_span: 1,
                            bounding_box: Rectangle {
                                x: 0,
                                y: 0,
                                width: 100,
                                height: 20,
                            },
                        },
                        TableCell {
                            content: "Edad".to_string(),
                            row_span: 1,
                            col_span: 1,
                            bounding_box: Rectangle {
                                x: 100,
                                y: 0,
                                width: 100,
                                height: 20,
                            },
                        },
                    ],
                    vec![
                        TableCell {
                            content: "Ana".to_string(),
                            row_span: 1,
                            col_span: 1,
                            bounding_box: Rectangle {
                                x: 0,
                                y: 20,
                                width: 100,
                                height: 20,
                            },
                        },
                        TableCell {
                            content: "30".to_string(),
                            row_span: 1,
                            col_span: 1,
                            bounding_box: Rectangle {
                                x: 100,
                                y: 20,
                                width: 100,
                                height: 20,
                            },
                        },
                    ],
                ],
            })
        } else {
            None
        };

        let doc = Document {
            id: "exp-test".to_string(),
            source_path: std::path::PathBuf::from("/tmp/exp.png"),
            pages: vec![Page {
                number: 1,
                dimensions: Dimensions {
                    width: 300,
                    height: 200,
                },
                image_data: Some(png_color_sintetico(300, 200, [240, 240, 240])),
                blocks: vec![
                    Block {
                        block_type: BlockType::Title,
                        bounding_box: Rectangle {
                            x: 20,
                            y: 10,
                            width: 260,
                            height: 30,
                        },
                        content: "Informe".to_string(),
                        confidence: 0.98,
                        embedded_image: None,
                        table_structure: None,
                        reading_order: 0,
                    },
                    Block {
                        block_type: BlockType::Table,
                        bounding_box: Rectangle {
                            x: 0,
                            y: 50,
                            width: 300,
                            height: 100,
                        },
                        content: "Nombre | Edad\nAna | 30".to_string(),
                        confidence: 0.9,
                        embedded_image: None,
                        table_structure: tabla,
                        reading_order: 1,
                    },
                    Block {
                        block_type: BlockType::Image,
                        bounding_box: Rectangle {
                            x: 40,
                            y: 155,
                            width: 80,
                            height: 35,
                        },
                        content: String::new(),
                        confidence: 0.95,
                        embedded_image: None,
                        table_structure: None,
                        reading_order: 2,
                    },
                ],
            }],
            metadata: HashMap::new(),
        };

        Job {
            id: "job-exp".to_string(),
            document: doc,
            status: JobStatus::Completed,
            created_at: std::time::SystemTime::now(),
            completed_at: Some(std::time::SystemTime::now()),
            profile: ProcessingProfile::Balanced,
            error_message: None,
            formato_salida: Default::default(),
        }
    }

    #[test]
    fn test_txt_exporter_tabla_con_estructura_usa_texto_plano() {
        let dir = std::env::temp_dir().join("ocrfast_exp_test_txt");
        std::fs::create_dir_all(&dir).unwrap();
        let ruta = dir.join("output.txt");

        let exporter = TxtExporter::new();
        exporter.export(&job_con_tabla(true), &ruta).unwrap();

        let contenido = std::fs::read_to_string(&ruta).unwrap();
        assert!(
            contenido.contains("Nombre\tEdad"),
            "La tabla estructurada debe degradarse a texto tabulado"
        );
        assert!(
            contenido.contains("ANA") || contenido.contains("Ana"),
            "El TXT debe incluir el contenido de celdas"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_docx_exporter_genera_paquete_word_con_media() {
        let dir = std::env::temp_dir().join("ocrfast_exp_test_docx");
        std::fs::create_dir_all(&dir).unwrap();
        let ruta = dir.join("output.docx");

        let exporter = DocxExporter::new();
        exporter.export(&job_con_tabla(true), &ruta).unwrap();

        assert!(ruta.exists(), "El archivo DOCX debe existir");
        let contenido = std::fs::read(&ruta).unwrap();
        assert!(
            contenido
                .windows("word/document.xml".len())
                .any(|w| w == b"word/document.xml"),
            "El paquete DOCX debe incluir word/document.xml"
        );
        assert!(
            contenido
                .windows("word/media/image1.png".len())
                .any(|w| w == b"word/media/image1.png"),
            "El DOCX debe incluir la imagen recortada como media interna"
        );
        assert!(
            contenido
                .windows("Informe".len())
                .any(|w| w == "Informe".as_bytes()),
            "El XML del documento debe contener el titulo exportado"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_latex_exporter_genera_fuente_y_assets() {
        let dir = std::env::temp_dir().join("ocrfast_exp_test_tex");
        std::fs::create_dir_all(&dir).unwrap();
        let ruta = dir.join("output.tex");

        let exporter = LatexExporter::new();
        exporter.export(&job_con_tabla(true), &ruta).unwrap();

        let contenido = std::fs::read_to_string(&ruta).unwrap();
        assert!(
            contenido.contains("\\begin{textblock*}"),
            "La salida LaTeX debe usar bloques absolutos guiados por blueprint"
        );
        assert!(
            contenido.contains("\\includegraphics"),
            "La salida LaTeX debe intentar preservar figuras del original"
        );
        assert!(
            contenido.contains("\\begin{tabular}"),
            "La salida LaTeX debe materializar tablas"
        );

        let assets = dir.join("output_assets");
        assert!(
            assets.exists(),
            "El exportador debe generar directorio de assets"
        );
        assert!(
            assets.join("page1_element3.png").exists(),
            "La figura recortada debe persistirse como asset externo"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_json_exporter_genera_json_valido() {
        let dir = std::env::temp_dir().join("ocrfast_exp_test_json");
        std::fs::create_dir_all(&dir).unwrap();
        let ruta = dir.join("output.json");

        let exporter = JsonExporter::new();
        exporter.export(&job_con_tabla(false), &ruta).unwrap();

        assert!(ruta.exists(), "El archivo JSON debe existir");
        let contenido = std::fs::read_to_string(&ruta).unwrap();
        let parsed: serde_json::Value =
            serde_json::from_str(&contenido).expect("El JSON generado debe ser valido");
        assert_eq!(parsed["id"], "job-exp");

        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod preprocessor_image_tests {
    use ocrfast::domain::{Dimensions, Document, Page};
    use ocrfast::infrastructure::preprocessors::ImagePreprocessor;
    use ocrfast::interfaces::ports::PreprocessorPort;

    fn png_gris_sintetico(ancho: u32, alto: u32, valor: u8) -> Vec<u8> {
        let img = image::GrayImage::from_pixel(ancho, alto, image::Luma([valor]));
        let mut buffer = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageLuma8(img)
            .write_to(&mut buffer, image::ImageFormat::Png)
            .unwrap();
        buffer.into_inner()
    }

    fn documento_con_imagen(imagen: Vec<u8>) -> Document {
        Document {
            id: "test-preproc".to_string(),
            source_path: std::path::PathBuf::from("/tmp/test.png"),
            pages: vec![Page {
                number: 1,
                dimensions: Dimensions {
                    width: 100,
                    height: 100,
                },
                blocks: vec![],
                image_data: Some(imagen),
            }],
            metadata: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn test_binarizacion_produce_imagen_valida() {
        let pp = ImagePreprocessor::with_config(true, false, false, 300);
        let imagen = png_gris_sintetico(100, 100, 128);
        let mut doc = documento_con_imagen(imagen);

        pp.preprocess(&mut doc)
            .expect("Binarizacion no debe fallar");

        let image_data = doc.pages[0].image_data.as_ref().unwrap();
        assert!(!image_data.is_empty());

        let img_resultado =
            image::load_from_memory(image_data).expect("La imagen binarizada debe ser PNG valido");
        assert_eq!(img_resultado.width(), 100);
        assert_eq!(img_resultado.height(), 100);
    }

    #[test]
    fn test_binarizacion_imagen_totalmente_blanca_no_panics() {
        let pp = ImagePreprocessor::with_config(true, false, false, 300);
        let imagen = png_gris_sintetico(50, 50, 255);
        let mut doc = documento_con_imagen(imagen);
        assert!(pp.preprocess(&mut doc).is_ok());
    }

    #[test]
    fn test_denoise_produce_imagen_valida() {
        let pp = ImagePreprocessor::with_config(false, false, true, 300);
        let imagen = png_gris_sintetico(80, 80, 200);
        let mut doc = documento_con_imagen(imagen);

        pp.preprocess(&mut doc).expect("Denoise no debe fallar");

        let image_data = doc.pages[0].image_data.as_ref().unwrap();
        let img = image::load_from_memory(image_data).unwrap();
        assert_eq!(img.width(), 80);
        assert_eq!(img.height(), 80);
    }

    #[test]
    fn test_deskew_imagen_sin_inclinacion_no_panics() {
        let pp = ImagePreprocessor::with_config(false, true, false, 300);
        let imagen = png_gris_sintetico(100, 150, 200);
        let mut doc = documento_con_imagen(imagen);
        assert!(pp.preprocess(&mut doc).is_ok());
    }

    #[test]
    fn test_pipeline_completo_no_panics() {
        let pp = ImagePreprocessor::new();
        let imagen = png_gris_sintetico(120, 160, 180);
        let mut doc = documento_con_imagen(imagen);

        pp.preprocess(&mut doc)
            .expect("Pipeline completo no debe fallar");

        let image_data = doc.pages[0].image_data.as_ref().unwrap();
        let img = image::load_from_memory(image_data).unwrap();
        assert_eq!(img.width(), 120);
        assert_eq!(img.height(), 160);
    }

    #[test]
    fn test_pagina_sin_imagen_se_omite_sin_error() {
        let pp = ImagePreprocessor::new();
        let mut doc = Document {
            id: "sin-img".to_string(),
            source_path: std::path::PathBuf::from("/tmp/test.pdf"),
            pages: vec![Page {
                number: 1,
                dimensions: Dimensions {
                    width: 100,
                    height: 100,
                },
                blocks: vec![],
                image_data: None,
            }],
            metadata: std::collections::HashMap::new(),
        };
        assert!(pp.preprocess(&mut doc).is_ok());
        assert!(doc.pages[0].image_data.is_none());
    }
}

#[cfg(test)]
mod layout_engine_tests {
    use ocrfast::domain::{Block, BlockType, Dimensions, Page};
    use ocrfast::infrastructure::layout_engines::XyCutLayoutEngine;
    use ocrfast::interfaces::ports::LayoutEnginePort;

    fn pagina_con_imagen_sintetica(ancho: u32, alto: u32) -> Page {
        let mut img = image::RgbImage::new(ancho, alto);
        for y in 0..alto {
            for x in 0..ancho {
                img.put_pixel(x, y, image::Rgb([255, 255, 255]));
            }
        }
        for y in 50..80 {
            for x in 50..ancho - 50 {
                img.put_pixel(x, y, image::Rgb([30, 30, 30]));
            }
        }
        for y in 150..300 {
            for x in 50..ancho - 50 {
                img.put_pixel(x, y, image::Rgb([40, 40, 40]));
            }
        }

        let mut buffer = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buffer, image::ImageFormat::Png)
            .unwrap();

        Page {
            number: 1,
            dimensions: Dimensions {
                width: ancho,
                height: alto,
            },
            blocks: vec![],
            image_data: Some(buffer.into_inner()),
        }
    }

    #[test]
    fn test_xy_cut_detecta_bloques_en_imagen_con_contenido() {
        let engine = XyCutLayoutEngine::new();
        let page = pagina_con_imagen_sintetica(600, 400);

        let bloques = engine.analyze(&page).expect("XY-Cut no debe fallar");

        assert!(!bloques.is_empty(), "Debe detectar al menos un bloque");
    }

    #[test]
    fn test_xy_cut_bloques_tienen_bounding_box_valido() {
        let engine = XyCutLayoutEngine::new();
        let page = pagina_con_imagen_sintetica(600, 400);

        let bloques = engine.analyze(&page).unwrap();

        for bloque in &bloques {
            assert!(
                bloque.bounding_box.width > 0,
                "Bloque debe tener ancho positivo"
            );
            assert!(
                bloque.bounding_box.height > 0,
                "Bloque debe tener alto positivo"
            );
            assert!(
                bloque.bounding_box.x + bloque.bounding_box.width <= 600,
                "Bloque no debe exceder ancho de pagina"
            );
            assert!(
                bloque.bounding_box.y + bloque.bounding_box.height <= 400,
                "Bloque no debe exceder alto de pagina"
            );
        }
    }

    #[test]
    fn test_xy_cut_asigna_orden_de_lectura_secuencial() {
        let engine = XyCutLayoutEngine::new();
        let page = pagina_con_imagen_sintetica(600, 400);

        let bloques = engine.analyze(&page).unwrap();

        for (i, bloque) in bloques.iter().enumerate() {
            assert_eq!(
                bloque.reading_order, i as u32,
                "reading_order debe ser secuencial"
            );
        }
    }

    #[test]
    fn test_xy_cut_pagina_sin_imagen_retorna_bloques_existentes() {
        let engine = XyCutLayoutEngine::new();
        let bloque_existente = Block {
            block_type: BlockType::Text,
            bounding_box: ocrfast::domain::Rectangle {
                x: 0,
                y: 0,
                width: 100,
                height: 50,
            },
            content: "texto previo".to_string(),
            confidence: 0.9,
            embedded_image: None,
            table_structure: None,
            reading_order: 0,
        };
        let page = Page {
            number: 1,
            dimensions: Dimensions {
                width: 600,
                height: 400,
            },
            blocks: vec![bloque_existente],
            image_data: None,
        };

        let bloques = engine.analyze(&page).unwrap();
        assert_eq!(bloques.len(), 1);
        assert_eq!(bloques[0].content, "texto previo");
    }

    #[test]
    fn test_xy_cut_imagen_totalmente_blanca_retorna_cero_bloques() {
        let engine = XyCutLayoutEngine::new();
        let mut buffer = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageLuma8(image::GrayImage::from_pixel(
            200,
            200,
            image::Luma([255u8]),
        ))
        .write_to(&mut buffer, image::ImageFormat::Png)
        .unwrap();

        let page = Page {
            number: 1,
            dimensions: Dimensions {
                width: 200,
                height: 200,
            },
            blocks: vec![],
            image_data: Some(buffer.into_inner()),
        };

        let bloques = engine.analyze(&page).unwrap();
        assert_eq!(bloques.len(), 0, "Imagen blanca no debe generar bloques");
    }
}

#[cfg(test)]
mod pipeline_cancelacion_tests {
    use ocrfast::application::pipeline::{OcrPipeline, MSG_JOB_CANCELADO};
    use ocrfast::domain::ProcessingProfile;
    use ocrfast::infrastructure::document_parsers::stub::StubDocumentParser;
    use ocrfast::infrastructure::ocr_engines::stub::StubOcrEngine;
    use std::path::Path;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    /// Verifica que un flag de cancelacion pre-activado detiene el pipeline tras el parseo.
    #[test]
    fn test_pipeline_cancelado_antes_de_primera_fase_retorna_error() {
        let pipeline =
            OcrPipeline::new(Arc::new(StubDocumentParser::new()), Arc::new(StubOcrEngine));

        let cancel_flag = Arc::new(AtomicBool::new(true)); // Pre-cancelado
        let ruta = Path::new("/tmp/doc_cancelar.pdf");

        let resultado = pipeline.procesar_documento(
            ruta,
            &ProcessingProfile::Balanced,
            None,
            Some(&cancel_flag),
        );

        assert!(resultado.is_err(), "Pipeline cancelado debe retornar error");
        assert_eq!(
            resultado.err().unwrap().to_string(),
            MSG_JOB_CANCELADO,
            "El mensaje de error debe ser exactamente MSG_JOB_CANCELADO"
        );
    }

    /// Verifica que sin flag de cancelacion el pipeline se completa normalmente.
    #[test]
    fn test_pipeline_sin_cancelacion_completa_correctamente() {
        let pipeline =
            OcrPipeline::new(Arc::new(StubDocumentParser::new()), Arc::new(StubOcrEngine));

        let resultado = pipeline.procesar_documento(
            Path::new("/tmp/doc_normal.pdf"),
            &ProcessingProfile::Balanced,
            None,
            None,
        );

        assert!(
            resultado.is_ok(),
            "Pipeline sin cancelacion debe completarse: {:?}",
            resultado.err()
        );
    }

    /// Verifica que activar el flag despues de iniciado no afecta un pipeline ya terminado.
    #[test]
    fn test_cancel_flag_activado_despues_no_afecta_pipeline_completado() {
        let pipeline =
            OcrPipeline::new(Arc::new(StubDocumentParser::new()), Arc::new(StubOcrEngine));

        let cancel_flag = Arc::new(AtomicBool::new(false));
        let flag_clone = Arc::clone(&cancel_flag);

        let resultado = pipeline.procesar_documento(
            Path::new("/tmp/doc_tarde.pdf"),
            &ProcessingProfile::Fast,
            None,
            Some(&cancel_flag),
        );

        flag_clone.store(true, Ordering::Relaxed);

        assert!(
            resultado.is_ok(),
            "Pipeline que termino antes de cancelar debe retornar Ok"
        );
    }
}

#[cfg(test)]
mod idioma_tests {
    use ocrfast::domain::LanguageConfig;

    /// Verifica que el idioma por defecto es espanol.
    #[test]
    fn test_idioma_default_es_spa() {
        let config = LanguageConfig::default();
        assert_eq!(config.primary, "spa");
        assert!(config.secondary.is_empty());
    }

    /// Verifica que el campo primary acepta cualquier codigo ISO 639-3.
    #[test]
    fn test_idioma_primary_se_puede_cambiar() {
        let mut config = LanguageConfig::default();
        config.primary = "eng".to_string();
        assert_eq!(config.primary, "eng");
    }
}
