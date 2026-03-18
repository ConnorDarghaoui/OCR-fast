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
            layout_confidence: None,
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
                    layout_confidence: None,
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
    fn test_file_job_store_crea_directorios_padre() {
        let directorio = tempfile::tempdir().expect("No se pudo crear directorio temporal");
        let ruta = directorio
            .path()
            .join("estado")
            .join("interno")
            .join("jobs.json");
        let store = FileJobStore::with_path(&ruta);

        store.save(&job_de_prueba("nested-1")).unwrap();

        assert!(ruta.exists());
        assert!(ruta.parent().unwrap().exists());
    }

    #[test]
    fn test_file_job_store_reporta_json_corrupto() {
        let directorio = tempfile::tempdir().expect("No se pudo crear directorio temporal");
        let ruta = directorio.path().join("jobs.json");
        std::fs::write(&ruta, "{ json invalido").expect("No se pudo escribir json corrupto");
        let store = FileJobStore::with_path(&ruta);

        let error = store
            .list()
            .expect_err("Se esperaba error por json corrupto");
        let mensaje = error.to_string();

        assert!(mensaje.contains("parseando jobs.json"));
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
                    layout_confidence: None,
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
    use encoding_rs::WINDOWS_1252;
    use ocrfast::domain::{
        Block, BlockType, Dimensions, Document, Job, JobStatus, Page, ProcessingProfile, Rectangle,
        TableCell, TableCellAlignment, TableCellStyle, TableStructure,
    };
    use ocrfast::infrastructure::document_assemblers::LayoutGuidedDocumentAssembler;
    use ocrfast::infrastructure::exporters::{
        DocxExporter, JsonExporter, LatexExporter, PdfReconstructedExporter, TxtExporter,
    };
    use ocrfast::interfaces::ports::{DocumentAssemblerPort, ExporterPort};
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
                header_row_indices: vec![0],
                column_widths: vec![140, 80],
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
                            style: Some(TableCellStyle {
                                alignment: TableCellAlignment::Center,
                                is_emphasized: true,
                            }),
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
                            style: Some(TableCellStyle {
                                alignment: TableCellAlignment::Center,
                                is_emphasized: true,
                            }),
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
                            style: Some(TableCellStyle {
                                alignment: TableCellAlignment::Left,
                                is_emphasized: false,
                            }),
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
                            style: Some(TableCellStyle {
                                alignment: TableCellAlignment::Right,
                                is_emphasized: false,
                            }),
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
                        layout_confidence: None,
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
                        layout_confidence: None,
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
                        layout_confidence: None,
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

    fn job_latex_semantico() -> Job {
        let tabla = Some(TableStructure {
            num_rows: 2,
            num_cols: 2,
            header_row_indices: vec![0],
            column_widths: vec![180, 220],
            rows: vec![
                vec![
                    TableCell {
                        content: "Seccion".to_string(),
                        row_span: 1,
                        col_span: 1,
                        bounding_box: Rectangle {
                            x: 0,
                            y: 0,
                            width: 180,
                            height: 30,
                        },
                        style: Some(TableCellStyle {
                            alignment: TableCellAlignment::Center,
                            is_emphasized: true,
                        }),
                    },
                    TableCell {
                        content: "Contenido".to_string(),
                        row_span: 1,
                        col_span: 1,
                        bounding_box: Rectangle {
                            x: 180,
                            y: 0,
                            width: 220,
                            height: 30,
                        },
                        style: Some(TableCellStyle {
                            alignment: TableCellAlignment::Center,
                            is_emphasized: true,
                        }),
                    },
                ],
                vec![
                    TableCell {
                        content: "Resumen".to_string(),
                        row_span: 1,
                        col_span: 1,
                        bounding_box: Rectangle {
                            x: 0,
                            y: 30,
                            width: 180,
                            height: 32,
                        },
                        style: Some(TableCellStyle {
                            alignment: TableCellAlignment::Left,
                            is_emphasized: false,
                        }),
                    },
                    TableCell {
                        content: "Tabla semantica lista para exportar".to_string(),
                        row_span: 1,
                        col_span: 1,
                        bounding_box: Rectangle {
                            x: 180,
                            y: 30,
                            width: 220,
                            height: 32,
                        },
                        style: Some(TableCellStyle {
                            alignment: TableCellAlignment::Left,
                            is_emphasized: false,
                        }),
                    },
                ],
            ],
        });

        Job {
            id: "job-latex-sem".to_string(),
            document: Document {
                id: "exp-latex-sem".to_string(),
                source_path: std::path::PathBuf::from("/tmp/latex-sem.pdf"),
                pages: vec![
                    Page {
                        number: 1,
                        dimensions: Dimensions {
                            width: 1200,
                            height: 1600,
                        },
                        image_data: Some(png_color_sintetico(1200, 1600, [251, 251, 245])),
                        blocks: vec![
                            Block {
                                block_type: BlockType::Text,
                                bounding_box: Rectangle {
                                    x: 120,
                                    y: 24,
                                    width: 960,
                                    height: 32,
                                },
                                content: "Encabezado repetido del libro".to_string(),
                                confidence: 0.99,
                                layout_confidence: None,
                                embedded_image: None,
                                table_structure: None,
                                reading_order: 0,
                            },
                            Block {
                                block_type: BlockType::Title,
                                bounding_box: Rectangle {
                                    x: 140,
                                    y: 120,
                                    width: 920,
                                    height: 64,
                                },
                                content: "Capitulo del libro".to_string(),
                                confidence: 0.98,
                                layout_confidence: None,
                                embedded_image: None,
                                table_structure: None,
                                reading_order: 1,
                            },
                            Block {
                                block_type: BlockType::Text,
                                bounding_box: Rectangle {
                                    x: 140,
                                    y: 240,
                                    width: 900,
                                    height: 180,
                                },
                                content: "Primer parrafo del documento reconstruido.".to_string(),
                                confidence: 0.96,
                                layout_confidence: None,
                                embedded_image: None,
                                table_structure: None,
                                reading_order: 2,
                            },
                        ],
                    },
                    Page {
                        number: 2,
                        dimensions: Dimensions {
                            width: 1200,
                            height: 1600,
                        },
                        image_data: Some(png_color_sintetico(1200, 1600, [252, 252, 246])),
                        blocks: vec![
                            Block {
                                block_type: BlockType::Text,
                                bounding_box: Rectangle {
                                    x: 120,
                                    y: 24,
                                    width: 960,
                                    height: 32,
                                },
                                content: "Encabezado repetido del libro".to_string(),
                                confidence: 0.99,
                                layout_confidence: None,
                                embedded_image: None,
                                table_structure: None,
                                reading_order: 0,
                            },
                            Block {
                                block_type: BlockType::Text,
                                bounding_box: Rectangle {
                                    x: 140,
                                    y: 140,
                                    width: 920,
                                    height: 160,
                                },
                                content: "Segundo parrafo del documento reconstruido.".to_string(),
                                confidence: 0.95,
                                layout_confidence: None,
                                embedded_image: None,
                                table_structure: None,
                                reading_order: 1,
                            },
                            Block {
                                block_type: BlockType::Table,
                                bounding_box: Rectangle {
                                    x: 140,
                                    y: 360,
                                    width: 760,
                                    height: 180,
                                },
                                content: "Seccion | Contenido\nResumen | Tabla semantica lista para exportar"
                                    .to_string(),
                                confidence: 0.93,
                                layout_confidence: None,
                                embedded_image: None,
                                table_structure: tabla.clone(),
                                reading_order: 2,
                            },
                        ],
                    },
                    Page {
                        number: 3,
                        dimensions: Dimensions {
                            width: 1200,
                            height: 1600,
                        },
                        image_data: Some(png_color_sintetico(1200, 1600, [245, 248, 252])),
                        blocks: vec![
                            Block {
                                block_type: BlockType::Text,
                                bounding_box: Rectangle {
                                    x: 120,
                                    y: 24,
                                    width: 960,
                                    height: 32,
                                },
                                content: "Encabezado repetido del libro".to_string(),
                                confidence: 0.99,
                                layout_confidence: None,
                                embedded_image: None,
                                table_structure: None,
                                reading_order: 0,
                            },
                            Block {
                                block_type: BlockType::Image,
                                bounding_box: Rectangle {
                                    x: 220,
                                    y: 180,
                                    width: 420,
                                    height: 260,
                                },
                                content: String::new(),
                                confidence: 0.97,
                                layout_confidence: None,
                                embedded_image: None,
                                table_structure: None,
                                reading_order: 1,
                            },
                        ],
                    },
                ],
                metadata: HashMap::new(),
            },
            status: JobStatus::Completed,
            created_at: std::time::SystemTime::now(),
            completed_at: Some(std::time::SystemTime::now()),
            profile: ProcessingProfile::Balanced,
            error_message: None,
            formato_salida: Default::default(),
        }
    }

    fn job_latex_facsimil() -> Job {
        let mut img = image::RgbImage::from_pixel(1200, 1600, image::Rgb([248, 248, 244]));
        for y in 180..430 {
            for x in 720..1020 {
                img.put_pixel(x, y, image::Rgb([210, 225, 245]));
            }
        }
        for y in 520..700 {
            for x in 140..520 {
                img.put_pixel(x, y, image::Rgb([245, 215, 200]));
            }
        }
        let mut buffer = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buffer, image::ImageFormat::Png)
            .unwrap();

        Job {
            id: "job-latex-fac".to_string(),
            document: Document {
                id: "exp-latex-fac".to_string(),
                source_path: std::path::PathBuf::from("/tmp/latex-fac.pdf"),
                pages: vec![Page {
                    number: 1,
                    dimensions: Dimensions {
                        width: 1200,
                        height: 1600,
                    },
                    image_data: Some(buffer.into_inner()),
                    blocks: vec![
                        Block {
                            block_type: BlockType::Text,
                            bounding_box: Rectangle {
                                x: 120,
                                y: 24,
                                width: 960,
                                height: 32,
                            },
                            content: "Encabezado repetido del libro".to_string(),
                            confidence: 0.99,
                            layout_confidence: None,
                            embedded_image: None,
                            table_structure: None,
                            reading_order: 0,
                        },
                        Block {
                            block_type: BlockType::Title,
                            bounding_box: Rectangle {
                                x: 120,
                                y: 120,
                                width: 960,
                                height: 70,
                            },
                            content: "Titulo original".to_string(),
                            confidence: 0.98,
                            layout_confidence: None,
                            embedded_image: None,
                            table_structure: None,
                            reading_order: 1,
                        },
                        Block {
                            block_type: BlockType::Text,
                            bounding_box: Rectangle {
                                x: 120,
                                y: 240,
                                width: 380,
                                height: 150,
                            },
                            content: "columna izquierda uno".to_string(),
                            confidence: 0.95,
                            layout_confidence: None,
                            embedded_image: None,
                            table_structure: None,
                            reading_order: 2,
                        },
                        Block {
                            block_type: BlockType::Text,
                            bounding_box: Rectangle {
                                x: 700,
                                y: 240,
                                width: 320,
                                height: 170,
                            },
                            content: "columna derecha dudosa".to_string(),
                            confidence: 0.43,
                            layout_confidence: None,
                            embedded_image: None,
                            table_structure: None,
                            reading_order: 3,
                        },
                        Block {
                            block_type: BlockType::Image,
                            bounding_box: Rectangle {
                                x: 140,
                                y: 520,
                                width: 380,
                                height: 180,
                            },
                            content: String::new(),
                            confidence: 0.97,
                            layout_confidence: None,
                            embedded_image: None,
                            table_structure: None,
                            reading_order: 4,
                        },
                    ],
                }],
                metadata: HashMap::new(),
            },
            status: JobStatus::Completed,
            created_at: std::time::SystemTime::now(),
            completed_at: Some(std::time::SystemTime::now()),
            profile: ProcessingProfile::Balanced,
            error_message: None,
            formato_salida: Default::default(),
        }
    }

    fn job_dos_columnas() -> Job {
        Job {
            id: "job-cols".to_string(),
            document: Document {
                id: "exp-cols".to_string(),
                source_path: std::path::PathBuf::from("/tmp/cols.png"),
                pages: vec![Page {
                    number: 1,
                    dimensions: Dimensions {
                        width: 1200,
                        height: 1600,
                    },
                    image_data: Some(png_color_sintetico(1200, 1600, [250, 250, 250])),
                    blocks: vec![
                        Block {
                            block_type: BlockType::Title,
                            bounding_box: Rectangle {
                                x: 120,
                                y: 60,
                                width: 960,
                                height: 90,
                            },
                            content: "Articulo".to_string(),
                            confidence: 0.99,
                            layout_confidence: None,
                            embedded_image: None,
                            table_structure: None,
                            reading_order: 0,
                        },
                        Block {
                            block_type: BlockType::Text,
                            bounding_box: Rectangle {
                                x: 120,
                                y: 240,
                                width: 380,
                                height: 180,
                            },
                            content: "columna izquierda uno".to_string(),
                            confidence: 0.95,
                            layout_confidence: None,
                            embedded_image: None,
                            table_structure: None,
                            reading_order: 1,
                        },
                        Block {
                            block_type: BlockType::Text,
                            bounding_box: Rectangle {
                                x: 120,
                                y: 460,
                                width: 380,
                                height: 180,
                            },
                            content: "columna izquierda dos".to_string(),
                            confidence: 0.95,
                            layout_confidence: None,
                            embedded_image: None,
                            table_structure: None,
                            reading_order: 2,
                        },
                        Block {
                            block_type: BlockType::Text,
                            bounding_box: Rectangle {
                                x: 700,
                                y: 240,
                                width: 380,
                                height: 180,
                            },
                            content: "columna derecha uno".to_string(),
                            confidence: 0.95,
                            layout_confidence: None,
                            embedded_image: None,
                            table_structure: None,
                            reading_order: 3,
                        },
                        Block {
                            block_type: BlockType::Text,
                            bounding_box: Rectangle {
                                x: 700,
                                y: 460,
                                width: 380,
                                height: 180,
                            },
                            content: "columna derecha dos".to_string(),
                            confidence: 0.95,
                            layout_confidence: None,
                            embedded_image: None,
                            table_structure: None,
                            reading_order: 4,
                        },
                    ],
                }],
                metadata: HashMap::new(),
            },
            status: JobStatus::Completed,
            created_at: std::time::SystemTime::now(),
            completed_at: Some(std::time::SystemTime::now()),
            profile: ProcessingProfile::Balanced,
            error_message: None,
            formato_salida: Default::default(),
        }
    }

    fn job_texto_acentuado() -> Job {
        Job {
            id: "job-acentos".to_string(),
            document: Document {
                id: "exp-acentos".to_string(),
                source_path: std::path::PathBuf::from("/tmp/acentos.png"),
                pages: vec![Page {
                    number: 1,
                    dimensions: Dimensions {
                        width: 1200,
                        height: 400,
                    },
                    image_data: Some(png_color_sintetico(1200, 400, [255, 255, 255])),
                    blocks: vec![Block {
                        block_type: BlockType::Text,
                        bounding_box: Rectangle {
                            x: 80,
                            y: 80,
                            width: 1000,
                            height: 80,
                        },
                        content: "Canción año útil Ñandú".to_string(),
                        confidence: 0.97,
                        layout_confidence: None,
                        embedded_image: None,
                        table_structure: None,
                        reading_order: 0,
                    }],
                }],
                metadata: HashMap::new(),
            },
            status: JobStatus::Completed,
            created_at: std::time::SystemTime::now(),
            completed_at: Some(std::time::SystemTime::now()),
            profile: ProcessingProfile::Balanced,
            error_message: None,
            formato_salida: Default::default(),
        }
    }

    fn job_pdf_fallback_confianza() -> Job {
        let mut img = image::RgbImage::from_pixel(800, 500, image::Rgb([255, 255, 255]));
        for y in 60..170 {
            for x in 60..360 {
                img.put_pixel(x, y, image::Rgb([240, 120, 120]));
            }
        }
        for y in 260..360 {
            for x in 60..420 {
                img.put_pixel(x, y, image::Rgb([180, 220, 255]));
            }
        }
        let mut buffer = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buffer, image::ImageFormat::Png)
            .unwrap();

        Job {
            id: "job-pdf-fallback".to_string(),
            document: Document {
                id: "exp-pdf-fallback".to_string(),
                source_path: std::path::PathBuf::from("/tmp/fallback.png"),
                pages: vec![Page {
                    number: 1,
                    dimensions: Dimensions {
                        width: 800,
                        height: 500,
                    },
                    image_data: Some(buffer.into_inner()),
                    blocks: vec![
                        Block {
                            block_type: BlockType::Text,
                            bounding_box: Rectangle {
                                x: 60,
                                y: 60,
                                width: 300,
                                height: 110,
                            },
                            content: "texto dudoso".to_string(),
                            confidence: 0.41,
                            layout_confidence: None,
                            embedded_image: None,
                            table_structure: None,
                            reading_order: 0,
                        },
                        Block {
                            block_type: BlockType::Text,
                            bounding_box: Rectangle {
                                x: 60,
                                y: 260,
                                width: 360,
                                height: 100,
                            },
                            content: "texto claro".to_string(),
                            confidence: 0.96,
                            layout_confidence: None,
                            embedded_image: None,
                            table_structure: None,
                            reading_order: 1,
                        },
                    ],
                }],
                metadata: HashMap::new(),
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
        assert!(
            contenido
                .windows("w:tblHeader".len())
                .any(|w| w == b"w:tblHeader")
                && contenido
                    .windows("w:gridCol".len())
                    .any(|w| w == b"w:gridCol"),
            "El DOCX debe materializar header lógico y grid de columnas"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_docx_exporter_materializa_banda_de_columnas() {
        let dir = std::env::temp_dir().join("ocrfast_exp_test_docx_cols");
        std::fs::create_dir_all(&dir).unwrap();
        let ruta = dir.join("output.docx");

        let exporter = DocxExporter::new();
        exporter.export(&job_dos_columnas(), &ruta).unwrap();

        let contenido = std::fs::read(&ruta).unwrap();
        assert!(
            contenido
                .windows("column-layout".len())
                .any(|window| window == b"column-layout"),
            "El DOCX debe incluir la banda columnar marcada en el XML"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_latex_exporter_semantico_genera_fuente_y_assets() {
        let dir = std::env::temp_dir().join("ocrfast_exp_test_tex");
        std::fs::create_dir_all(&dir).unwrap();
        let ruta = dir.join("output.tex");

        let exporter = LatexExporter::new();
        exporter.export(&job_latex_semantico(), &ruta).unwrap();

        let contenido = std::fs::read_to_string(&ruta).unwrap();
        assert!(
            contenido.contains("\\section*{Capitulo del libro}"),
            "La salida LaTeX semantica debe materializar titulos como secciones"
        );
        assert!(
            contenido.contains("\\includegraphics"),
            "La salida LaTeX debe preservar figuras del original"
        );
        assert!(
            contenido.contains("\\begin{tabular}"),
            "La salida LaTeX semantica debe materializar tablas"
        );
        assert!(
            !contenido.contains("Encabezado repetido del libro"),
            "Los headers sospechosos no deben contaminar el cuerpo semantico"
        );
        assert!(
            !contenido.contains("\\begin{textblock*}"),
            "La ruta semantica no debe depender de textblock absoluto"
        );

        let assets = dir.join("output_assets");
        assert!(
            assets.exists(),
            "El exportador debe generar directorio de assets"
        );
        assert!(
            std::fs::read_dir(&assets)
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| entry.path().extension().is_some_and(|ext| ext == "png")),
            "La figura recortada debe persistirse como asset externo"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_latex_exporter_facsimil_preserva_geometria_y_fallback_raster() {
        let dir = std::env::temp_dir().join("ocrfast_exp_test_tex_fac");
        std::fs::create_dir_all(&dir).unwrap();
        let ruta = dir.join("output.tex");

        let exporter = LatexExporter::new_facsimile();
        exporter.export(&job_latex_facsimil(), &ruta).unwrap();

        let contenido = std::fs::read_to_string(&ruta).unwrap();
        assert!(
            contenido.contains("\\begin{textblock*}"),
            "La salida facsimil debe posicionar bloques absolutos"
        );
        assert!(
            contenido.contains("Encabezado repetido del libro"),
            "La ruta facsimil debe preservar headers repetidos visibles"
        );
        assert!(
            !contenido.contains("\\section*"),
            "La ruta facsimil no debe convertir titulos a secciones semanticas"
        );
        assert!(
            contenido.contains("(57.60pt,115.20pt)") && contenido.contains("(336.00pt,115.20pt)"),
            "La ruta facsimil debe preservar la geometria de columnas"
        );
        assert!(
            !contenido.contains("columna derecha dudosa"),
            "El texto de baja confianza debe degradarse a recorte raster"
        );
        assert!(
            contenido.matches("\\includegraphics").count() >= 2,
            "La salida facsimil debe incluir la figura y el fallback raster"
        );

        let assets = dir.join("output_assets");
        let cantidad_png = std::fs::read_dir(&assets)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "png"))
            .count();
        assert!(
            cantidad_png >= 2,
            "La ruta facsimil debe persistir assets para figura y fallback raster"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_pdf_reconstructed_exporter_genera_pdf_visible_con_xobject() {
        let dir = std::env::temp_dir().join("ocrfast_exp_test_pdf");
        std::fs::create_dir_all(&dir).unwrap();
        let ruta = dir.join("output.pdf");

        let exporter = PdfReconstructedExporter::new();
        exporter.export(&job_con_tabla(true), &ruta).unwrap();

        assert!(ruta.exists(), "El archivo PDF debe existir");

        let pdf = lopdf::Document::load(&ruta).expect("El PDF generado debe abrirse");
        let paginas = pdf.get_pages();
        assert_eq!(
            paginas.len(),
            1,
            "La salida de prueba debe tener una pagina"
        );

        let (&_numero_pagina, &page_id) = paginas
            .iter()
            .next()
            .expect("El PDF debe contener una pagina accesible");

        let contenido = pdf
            .get_page_content(page_id)
            .expect("El content stream debe poder decodificarse");
        let contenido_pdf = String::from_utf8_lossy(&contenido);
        assert!(
            contenido_pdf.contains(" Tj")
                || contenido_pdf.contains(" TJ")
                || contenido_pdf.contains("Tj\n"),
            "El PDF reconstruido debe contener operaciones de texto visibles"
        );
        assert!(
            !contenido_pdf.contains(" Tr "),
            "El PDF reconstruido no debe depender de texto invisible"
        );

        let pagina = pdf
            .get_dictionary(page_id)
            .expect("La pagina debe exponer su diccionario");
        let recursos_ref = pagina
            .get(b"Resources")
            .expect("La pagina debe tener recursos");
        let recursos = recursos_ref
            .as_reference()
            .ok()
            .and_then(|id| pdf.get_dictionary(id).ok().cloned())
            .or_else(|| recursos_ref.as_dict().ok().cloned())
            .expect("Los recursos de la pagina deben resolverse");
        let xobjects = recursos
            .get(b"XObject")
            .ok()
            .and_then(|obj| obj.as_dict().ok())
            .expect("El PDF reconstruido debe registrar al menos un XObject");
        assert!(
            !xobjects.is_empty(),
            "La pagina debe contener el recorte de imagen del original"
        );
        let primer_xobject_id = xobjects
            .iter()
            .next()
            .and_then(|(_nombre, objeto)| objeto.as_reference().ok())
            .expect("El XObject debe resolverse como referencia");
        let stream = pdf
            .get_object(primer_xobject_id)
            .expect("El XObject debe existir")
            .as_stream()
            .expect("El XObject debe ser un stream de imagen");
        assert_eq!(
            stream
                .dict
                .get(b"Filter")
                .expect("La imagen debe declarar un filtro")
                .as_name()
                .expect("El filtro debe ser un nombre"),
            b"DCTDecode"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_pdf_reconstructed_exporter_codifica_texto_winansi() {
        let dir = std::env::temp_dir().join("ocrfast_exp_test_pdf_unicode");
        std::fs::create_dir_all(&dir).unwrap();
        let ruta = dir.join("output.pdf");

        let exporter = PdfReconstructedExporter::new();
        exporter.export(&job_texto_acentuado(), &ruta).unwrap();

        let pdf = lopdf::Document::load(&ruta).expect("El PDF generado debe abrirse");
        let paginas = pdf.get_pages();
        let (&_numero_pagina, &page_id) = paginas
            .iter()
            .next()
            .expect("El PDF debe contener una pagina accesible");

        let contenido = pdf
            .get_page_content(page_id)
            .expect("El content stream debe leerse");
        let content = lopdf::content::Content::decode(&contenido)
            .expect("El content stream debe decodificarse");
        let texto_operacion = content
            .operations
            .iter()
            .find(|op| op.operator == "Tj")
            .and_then(|op| op.operands.first())
            .and_then(|obj| obj.as_str().ok())
            .expect("Debe existir al menos una operacion Tj con bytes de texto");
        let (texto_decodificado, _, _) = WINDOWS_1252.decode(texto_operacion);
        assert_eq!(texto_decodificado, "Canción año útil Ñandú");

        let pagina = pdf
            .get_dictionary(page_id)
            .expect("La pagina debe exponer su diccionario");
        let recursos_ref = pagina
            .get(b"Resources")
            .expect("La pagina debe tener recursos");
        let recursos = recursos_ref
            .as_reference()
            .ok()
            .and_then(|id| pdf.get_dictionary(id).ok().cloned())
            .or_else(|| recursos_ref.as_dict().ok().cloned())
            .expect("Los recursos de la pagina deben resolverse");
        let fuentes = recursos
            .get(b"Font")
            .expect("La pagina debe exponer fuentes")
            .as_dict()
            .expect("El diccionario de fuentes debe ser directo");
        let fuente = fuentes
            .get(b"F1")
            .expect("La fuente F1 debe existir")
            .as_reference()
            .ok()
            .and_then(|id| pdf.get_dictionary(id).ok().cloned())
            .expect("La fuente F1 debe resolverse");
        assert_eq!(
            fuente
                .get(b"Encoding")
                .expect("La fuente debe declarar Encoding")
                .as_name()
                .expect("El encoding debe ser un name"),
            b"WinAnsiEncoding"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_pdf_reconstructed_exporter_preserva_imagen_en_bloque_de_baja_confianza() {
        let dir = std::env::temp_dir().join("ocrfast_exp_test_pdf_confidence_fallback");
        std::fs::create_dir_all(&dir).unwrap();
        let ruta = dir.join("output.pdf");

        let exporter = PdfReconstructedExporter::new();
        exporter
            .export(&job_pdf_fallback_confianza(), &ruta)
            .unwrap();

        let pdf = lopdf::Document::load(&ruta).expect("El PDF generado debe abrirse");
        let (&_numero_pagina, &page_id) = pdf
            .get_pages()
            .iter()
            .next()
            .expect("El PDF debe contener una pagina accesible");
        let contenido = pdf
            .get_page_content(page_id)
            .expect("El content stream debe leerse");
        let content = lopdf::content::Content::decode(&contenido)
            .expect("El content stream debe decodificarse");

        let operaciones_do = content
            .operations
            .iter()
            .filter(|op| op.operator == "Do")
            .count();
        assert_eq!(
            operaciones_do, 1,
            "Solo el bloque de baja confianza debe degradarse a imagen"
        );

        let textos: Vec<String> = content
            .operations
            .iter()
            .filter(|op| op.operator == "Tj")
            .filter_map(|op| op.operands.first())
            .filter_map(|obj| obj.as_str().ok())
            .map(|bytes| WINDOWS_1252.decode(bytes).0.into_owned())
            .collect();
        let texto_visible = textos.join(" ");
        assert!(
            texto_visible.contains("texto") && texto_visible.contains("claro"),
            "El bloque de alta confianza debe seguir saliendo como texto visible"
        );
        assert!(
            !texto_visible.contains("dudoso"),
            "El bloque de baja confianza no debe serializarse como texto PDF"
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

    #[test]
    fn test_txt_exporta_en_orden_de_lectura_canonico() {
        let dir = std::env::temp_dir().join("ocrfast_exp_test_txt_order");
        std::fs::create_dir_all(&dir).unwrap();
        let ruta = dir.join("output.txt");

        let mut job = job_con_tabla(false);
        job.document.pages[0].blocks = vec![
            Block {
                block_type: BlockType::Text,
                bounding_box: Rectangle {
                    x: 180,
                    y: 20,
                    width: 80,
                    height: 20,
                },
                content: "Columna derecha".to_string(),
                confidence: 0.9,
                layout_confidence: None,
                embedded_image: None,
                table_structure: None,
                reading_order: 9,
            },
            Block {
                block_type: BlockType::Title,
                bounding_box: Rectangle {
                    x: 0,
                    y: 0,
                    width: 280,
                    height: 20,
                },
                content: "Titulo".to_string(),
                confidence: 0.95,
                layout_confidence: None,
                embedded_image: None,
                table_structure: None,
                reading_order: 4,
            },
            Block {
                block_type: BlockType::Text,
                bounding_box: Rectangle {
                    x: 10,
                    y: 20,
                    width: 80,
                    height: 20,
                },
                content: "Columna izquierda".to_string(),
                confidence: 0.9,
                layout_confidence: None,
                embedded_image: None,
                table_structure: None,
                reading_order: 7,
            },
        ];

        LayoutGuidedDocumentAssembler::new()
            .assemble(&mut job.document)
            .unwrap();

        let exporter = TxtExporter::new();
        exporter.export(&job, &ruta).unwrap();

        let contenido = std::fs::read_to_string(&ruta).unwrap();
        let indice_titulo = contenido.find("TITULO").unwrap();
        let indice_izquierda = contenido.find("Columna izquierda").unwrap();
        let indice_derecha = contenido.find("Columna derecha").unwrap();

        assert!(indice_titulo < indice_izquierda);
        assert!(indice_izquierda < indice_derecha);

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
            layout_confidence: None,
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
    use ocrfast::application::pipeline::{OcrPipeline, PipelineFailure};
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
        assert!(
            matches!(resultado.err().unwrap(), PipelineFailure::Cancelado),
            "La cancelacion debe expresarse con una variante tipada"
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
