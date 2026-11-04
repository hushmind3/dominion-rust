const ko = {
  'video.title': '영상 제작',
  'video.description': '영상 모델을 선택해 쇼츠를 생성하고 YouTube에 올립니다.',
  'video.tab.create': '영상 생성',
  'video.tab.settings': '설정',
  'video.refresh': '상태 새로고침',
  'video.refreshing': '새로고침 중',
  'video.youtube.connect': 'YouTube 계정 연동',
  'video.engine': '영상 엔진',
  'video.engine.save': '엔진 저장',
  'video.model': '모델',
  'video.model.pick': '모델 선택',
  'video.prompt': '프롬프트',
  'video.generate': '영상 생성',
  'video.result': '생성 결과',
  'video.upload': 'YouTube 업로드',
} as const
export type LocaleKey = keyof typeof ko
export default ko
